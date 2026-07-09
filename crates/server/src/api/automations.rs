use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use localflow_core::{
    db::models::{Automation, RunView},
    lua::{find_example, EXAMPLES},
    AutomationInput, CoreError,
};
use minijinja::context;
use serde::{Deserialize, Serialize};

use super::{is_htmx, redirect};
use crate::{errors::AppResult, state::AppState};

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

    fn to_input(&self) -> AutomationInput {
        AutomationInput {
            name: self.name.clone(),
            description: self.description.clone(),
            lua_code: self.lua_code.clone(),
            schedule: Some(self.schedule.clone()),
            enabled: self.enabled.is_some(),
        }
    }
}

#[derive(Serialize)]
struct DashboardRow {
    #[serde(flatten)]
    automation: Automation,
    last_run: Option<RunView>,
}

pub async fn dashboard(State(state): State<AppState>) -> AppResult<Response> {
    let rows: Vec<DashboardRow> = state
        .flow
        .list()
        .await?
        .into_iter()
        .map(|s| DashboardRow { automation: s.automation, last_run: s.last_run.map(RunView::from) })
        .collect();

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
) -> AppResult<Html<String>> {
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
    match state.flow.create(&form.to_input()).await {
        Ok(automation) => Ok(redirect(&headers, &format!("/automations/{}", automation.id))),
        Err(CoreError::Validation(errors)) => {
            let page = render_form(&state, &form, None, &errors)?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response())
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    Ok(render_detail(&state, id, None).await?.into_response())
}

/// The details page, optionally with the result of a run that just finished shown at the top.
async fn render_detail(
    state: &AppState,
    id: i64,
    last_result: Option<RunView>,
) -> AppResult<Html<String>> {
    let automation = state.flow.get(id).await?;
    let runs: Vec<RunView> = state.flow.runs(id, 10).await?.into_iter().map(Into::into).collect();
    let logs = state.flow.logs(id, 20).await?;
    let scheduled = state.flow.is_scheduled(id).await;

    state.render(
        "detail.html",
        context! { automation, runs, logs, scheduled, last_result },
    )
}

pub async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let automation = state.flow.get(id).await?;
    let form = AutomationForm::from_automation(&automation);
    Ok(render_form(&state, &form, Some(id), &[])?.into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<AutomationForm>,
) -> AppResult<Response> {
    state.flow.get(id).await?;

    match state.flow.update(id, &form.to_input()).await {
        Ok(_) => Ok(redirect(&headers, &format!("/automations/{id}"))),
        Err(CoreError::Validation(errors)) => {
            let page = render_form(&state, &form, Some(id), &errors)?;
            // HTMX only swaps 2xx responses, so let it show the errors.
            let status = if is_htmx(&headers) { StatusCode::OK } else { StatusCode::UNPROCESSABLE_ENTITY };
            Ok((status, page).into_response())
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let run = state.flow.run(id, "manual").await?;

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
    state.flow.toggle(id).await?;

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
    state.flow.delete(id).await?;
    Ok(redirect(&headers, "/"))
}
