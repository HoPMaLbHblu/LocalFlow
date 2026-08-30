use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Form,
};
use minijinja::context;
use serde::{Deserialize, Serialize};

use super::{is_htmx, redirect};
use crate::{
    db::models::{Automation, NewAutomation, RunView},
    errors::{AppError, AppResult},
    lua::{engine, find_example, EXAMPLES},
    scheduler::validate_cron,
    state::AppState,
};

/// Fields submitted by the create/edit form.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct AutomationForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub lua_code: String,
    #[serde(default)]
    pub schedule: String,
    /// Checkbox: present ("on") when ticked, missing otherwise.
    #[serde(default)]
    pub enabled: Option<String>,
}

impl AutomationForm {
    fn from_automation(a: &Automation) -> Self {
        AutomationForm {
            name: a.name.clone(),
            description: a.description.clone(),
            lua_code: a.lua_code.clone(),
            schedule: a.schedule.clone().unwrap_or_default(),
            enabled: a.enabled.then(|| "on".to_string()),
        }
    }

    /// Check every field and collect all problems, so the user can fix them in one go.
    fn validate(&self) -> Result<NewAutomation, Vec<String>> {
        let mut errors = Vec::new();

        let name = self.name.trim();
        if name.is_empty() {
            errors.push("Name is required.".to_string());
        } else if name.chars().count() > 100 {
            errors.push("Name must be at most 100 characters.".to_string());
        }

        if let Err(e) = engine::validate(&self.lua_code) {
            errors.push(e);
        }

        let schedule = self.schedule.trim();
        if !schedule.is_empty() {
            if let Err(e) = validate_cron(schedule) {
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(NewAutomation {
            name: name.to_string(),
            description: self.description.trim().to_string(),
            lua_code: self.lua_code.clone(),
            schedule: (!schedule.is_empty()).then(|| schedule.to_string()),
            enabled: self.enabled.is_some(),
        })
    }
}

async fn find(state: &AppState, id: i64) -> AppResult<Automation> {
    state.repo.get_automation(id).await?.ok_or(AppError::NotFound)
}

#[derive(Serialize)]
struct DashboardRow {
    #[serde(flatten)]
    automation: Automation,
    last_run: Option<RunView>,
}

pub async fn dashboard(State(state): State<AppState>) -> AppResult<Response> {
    let mut rows = Vec::new();
    for automation in state.repo.list_automations().await? {
        let last_run = state.repo.latest_run(automation.id).await?.map(RunView::from);
        rows.push(DashboardRow { automation, last_run });
    }

    let enabled = rows.iter().filter(|r| r.automation.enabled).count();
    let scheduled = rows
        .iter()
        .filter(|r| r.automation.enabled && r.automation.schedule.is_some())
        .count();

    Ok(state
        .render(
            "dashboard.html",
            context! { rows, enabled, scheduled, examples => EXAMPLES },
        )?
        .into_response())
}

fn render_form(
    state: &AppState,
    form: &AutomationForm,
    automation_id: Option<i64>,
    errors: &[String],
) -> AppResult<axum::response::Html<String>> {
    state.render(
        "form.html",
        context! {
            form,
            automation_id,
            errors,
            examples => EXAMPLES,
        },
    )
}

#[derive(Deserialize)]
pub struct NewQuery {
    template: Option<String>,
}

pub async fn new_form(
    State(state): State<AppState>,
    Query(query): Query<NewQuery>,
) -> AppResult<Response> {
    let form = match query.template.as_deref().and_then(find_example) {
        Some(example) => AutomationForm {
            name: example.title.to_string(),
            description: example.description.to_string(),
            lua_code: example.code.to_string(),
            schedule: example.schedule.to_string(),
            enabled: Some("on".into()),
        },
        None => AutomationForm {
            lua_code: find_example("hello-world").map(|e| e.code.to_string()).unwrap_or_default(),
            enabled: Some("on".into()),
            ..Default::default()
        },
    };
    Ok(render_form(&state, &form, None, &[])?.into_response())
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AutomationForm>,
) -> AppResult<Response> {
    let new = match form.validate() {
        Ok(new) => new,
        Err(errors) => {
            let page = render_form(&state, &form, None, &errors)?;
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response());
        }
    };

    let automation = state.repo.create_automation(&new).await?;
    state.scheduler.sync(&state, &automation).await?;
    tracing::info!(automation_id = automation.id, "created automation '{}'", automation.name);

    Ok(redirect(&headers, &format!("/automations/{}", automation.id)))
}

pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    Ok(render_detail(&state, id, None).await?.into_response())
}

/// The details page, optionally with the result of a run that just finished shown at the top.
async fn render_detail(
    state: &AppState,
    id: i64,
    last_result: Option<RunView>,
) -> AppResult<axum::response::Html<String>> {
    let automation = find(state, id).await?;
    let runs: Vec<RunView> = state.repo.list_runs(id, 10).await?.into_iter().map(Into::into).collect();
    let logs = state.repo.list_logs(id, 20).await?;
    let scheduled = state.scheduler.is_scheduled(id).await;

    state.render(
        "detail.html",
        context! { automation, runs, logs, scheduled, last_result },
    )
}

pub async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = find(&state, id).await?;
    let form = AutomationForm::from_automation(&automation);
    Ok(render_form(&state, &form, Some(id), &[])?.into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<AutomationForm>,
) -> AppResult<Response> {
    find(&state, id).await?;

    let new = match form.validate() {
        Ok(new) => new,
        Err(errors) => {
            let page = render_form(&state, &form, Some(id), &errors)?;
            // HTMX only swaps 2xx responses, so let it show the errors.
            let status = if is_htmx(&headers) { StatusCode::OK } else { StatusCode::UNPROCESSABLE_ENTITY };
            return Ok((status, page).into_response());
        }
    };

    let automation = state.repo.update_automation(id, &new).await?.ok_or(AppError::NotFound)?;
    state.scheduler.sync(&state, &automation).await?;
    tracing::info!(automation_id = id, "updated automation");

    Ok(redirect(&headers, &format!("/automations/{id}")))
}

pub async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let run = engine::run_automation(&state, id, "manual").await?;

    if is_htmx(&headers) {
        // Re-render the whole page so run history and logs are current too.
        Ok(render_detail(&state, id, Some(RunView::from(run))).await?.into_response())
    } else {
        Ok(redirect(&headers, &format!("/automations/{id}")))
    }
}

#[derive(Deserialize)]
pub struct ToggleForm {
    /// Where to go afterwards (e.g. "/" when toggled from the dashboard).
    next: Option<String>,
}

pub async fn toggle(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<ToggleForm>,
) -> AppResult<Response> {
    let current = find(&state, id).await?;
    let automation = state
        .repo
        .set_enabled(id, !current.enabled)
        .await?
        .ok_or(AppError::NotFound)?;
    state.scheduler.sync(&state, &automation).await?;
    tracing::info!(automation_id = id, enabled = automation.enabled, "toggled automation");

    // Only follow local paths, never another site.
    let next = form
        .next
        .filter(|n| n.starts_with('/') && !n.starts_with("//"))
        .unwrap_or_else(|| format!("/automations/{id}"));
    Ok(redirect(&headers, &next))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    state.scheduler.remove(id).await?;
    if !state.repo.delete_automation(id).await? {
        return Err(AppError::NotFound);
    }
    tracing::info!(automation_id = id, "deleted automation");
    Ok(redirect(&headers, "/"))
}
