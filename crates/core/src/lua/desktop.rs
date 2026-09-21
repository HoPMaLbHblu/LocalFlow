//! Windows settings: theme, transparency, accent colour, wallpaper style, exact volume,
//! power plans and timers, mouse speed, and Explorer's hidden files and extensions.
//! Reading is always allowed; changing needs "Allow system control".
//!
//! ```lua
//! desktop.dark_mode()                 desktop.set_dark_mode(true)
//! desktop.transparency()              desktop.set_transparency(false)
//! desktop.accent_color()  -> "#0078d4"   desktop.set_accent_color("#e81123")
//! desktop.wallpaper()     -> path        desktop.set_wallpaper(path, "fill")   -- fill, fit, stretch, center, tile, span
//! system.volume() -> 0..100            system.set_volume(35)
//! system.muted()  -> true/false        system.set_mute(true)
//! power.plans()  -> { { name, id, active } }   power.plan() -> "balanced"   power.set_plan("high performance")
//! power.set_screen_off(minutes, "plugged" | "battery" | nil)   power.set_sleep(minutes, ...)   (0 = never)
//! mouse.speed() -> 1..20              mouse.set_speed(10)
//! explorer.hidden_files()             explorer.set_hidden_files(true)
//! explorer.file_extensions()          explorer.set_file_extensions(true)
//! ```

use std::sync::Arc;

use mlua::{Lua, Table, Value};

use super::sandbox::PathPolicy;

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn require(allowed: bool, function: &str) -> mlua::Result<()> {
    if allowed {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!(
            "{function} needs \"Allow system control\". Switch it on for this automation in the editor, \
             and only for scripts you trust."
        )))
    }
}

/// "#0078d4" -> (r, g, b)
pub fn parse_color(text: &str) -> Result<(u8, u8, u8), String> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("\"{text}\" is not a colour like \"#0078d4\""));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
    Ok((byte(0)?, byte(2)?, byte(4)?))
}

pub fn format_color((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The built-in power plans, whose ids are the same on every PC.
pub const POWER_PLANS: &[(&str, &str)] = &[
    ("balanced", "381b4222-f694-41f0-9685-ff5bb260df2e"),
    ("high performance", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
    ("power saver", "a1841308-3541-4fab-bc81-f71556f20b4a"),
    ("ultimate performance", "e9a42b02-d5df-448d-aa00-03f14749eb61"),
];

/// Lines of `powercfg /list`: "Power Scheme GUID: <id>  (<name>) *" (the name is in the PC's language).
pub fn parse_power_plans(text: &str) -> Vec<(String, String, bool)> {
    text.lines()
        .filter_map(|line| {
            let colon = line.find(':')?;
            let rest = line[colon + 1..].trim();
            let id = rest.split_whitespace().next()?.to_lowercase();
            if id.len() != 36 {
                return None;
            }
            let name = rest.split_once('(').and_then(|(_, n)| n.split_once(')')).map(|(n, _)| n.trim().to_string())?;
            // Built-in plans get their English name so scripts work in every language.
            let name = POWER_PLANS.iter().find(|(_, g)| *g == id).map(|(n, _)| n.to_string()).unwrap_or(name);
            Some((name, id, rest.trim_end().ends_with('*')))
        })
        .collect()
}

#[cfg(windows)]
mod win {
    use std::process::Command;

    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, LPARAM},
        System::Registry::{
            RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
        },
        UI::WindowsAndMessaging::{
            SendMessageTimeoutW, SystemParametersInfoW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
            SPI_GETDESKWALLPAPER, SPI_GETMOUSESPEED, SPI_SETDESKWALLPAPER, SPI_SETMOUSESPEED, WM_SETTINGCHANGE,
        },
    };

    pub const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    pub const DWM: &str = r"Software\Microsoft\Windows\DWM";
    pub const EXPLORER_ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
    pub const DESKTOP: &str = r"Control Panel\Desktop";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read_dword(key: &str, name: &str) -> Option<u32> {
        let (key, name) = (wide(key), wide(name));
        let mut value = 0u32;
        let mut size = 4u32;
        // SAFETY: valid NUL-terminated strings; `value` has room for a DWORD.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER as HKEY, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut value as *mut u32).cast(), &mut size)
        };
        (status == ERROR_SUCCESS).then_some(value)
    }

    pub fn write_dword(key: &str, name: &str, value: u32) -> Result<(), String> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: valid NUL-terminated strings; the data is one DWORD.
        let status = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER as HKEY, key.as_ptr(), name.as_ptr(), REG_DWORD, (&value as *const u32).cast(), 4)
        };
        if status == ERROR_SUCCESS { Ok(()) } else { Err(format!("Windows refused the change (error {status})")) }
    }

    pub fn write_string(key: &str, name: &str, value: &str) -> Result<(), String> {
        let (key, name, data) = (wide(key), wide(name), wide(value));
        // SAFETY: valid NUL-terminated strings; the size is in bytes including the NUL.
        let status = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER as HKEY, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
        };
        if status == ERROR_SUCCESS { Ok(()) } else { Err(format!("Windows refused the change (error {status})")) }
    }

    #[allow(dead_code)]
    pub fn read_string(key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` has `size` bytes of room.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER as HKEY, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size)
        };
        (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
    }

    /// Tell open windows that a setting changed, so the taskbar and apps update.
    pub fn broadcast(area: &str) {
        let area = wide(area);
        let mut result = 0usize;
        // SAFETY: `area` outlives the call; a hung window can't block us (SMTO_ABORTIFHUNG, 1 s).
        unsafe {
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, area.as_ptr() as LPARAM, SMTO_ABORTIFHUNG, 1000, &mut result);
        }
    }

    pub fn mouse_speed() -> u32 {
        let mut speed = 0u32;
        // SAFETY: SPI_GETMOUSESPEED writes one u32.
        unsafe { SystemParametersInfoW(SPI_GETMOUSESPEED, 0, (&mut speed as *mut u32).cast(), 0) };
        speed
    }

    pub fn set_mouse_speed(speed: u32) -> Result<(), String> {
        // SAFETY: for SPI_SETMOUSESPEED the value is passed in the pointer argument itself.
        let ok = unsafe { SystemParametersInfoW(SPI_SETMOUSESPEED, 0, speed as usize as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
        if ok == 0 { Err("Windows refused the change".into()) } else { Ok(()) }
    }

    pub fn wallpaper() -> String {
        let mut buffer = vec![0u16; 1024];
        // SAFETY: the buffer holds 1024 characters.
        unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buffer.len() as u32, buffer.as_mut_ptr().cast(), 0) };
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    pub fn set_wallpaper(path: &std::path::Path, style: &str) -> Result<(), String> {
        // WallpaperStyle / TileWallpaper, as the Settings app stores them.
        let (wallpaper_style, tile) = match style {
            "fill" => ("10", "0"),
            "fit" => ("6", "0"),
            "stretch" => ("2", "0"),
            "center" => ("0", "0"),
            "tile" => ("0", "1"),
            "span" => ("22", "0"),
            other => return Err(format!("unknown style \"{other}\": use fill, fit, stretch, center, tile or span")),
        };
        write_string(DESKTOP, "WallpaperStyle", wallpaper_style)?;
        write_string(DESKTOP, "TileWallpaper", tile)?;
        let path = wide(&path.to_string_lossy());
        // SAFETY: `path` is a NUL-terminated string that outlives the call.
        let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, path.as_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
        if ok == 0 { Err("Windows refused this picture".into()) } else { Ok(()) }
    }

    fn with_endpoint<T>(f: impl FnOnce(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> windows::core::Result<T>) -> Result<T, String> {
        use windows::Win32::{
            Media::Audio::{eConsole, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator},
            System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
        };
        // SAFETY: standard Core Audio calls; COM is initialised on this thread first.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|_| "no speakers or headphones found".to_string())?;
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
            f(&volume).map_err(|e| e.to_string())
        }
    }

    pub fn volume() -> Result<u32, String> {
        // SAFETY: plain getter on a valid interface.
        with_endpoint(|v| unsafe { v.GetMasterVolumeLevelScalar() }).map(|level| (level * 100.0).round() as u32)
    }

    pub fn set_volume(percent: u32) -> Result<(), String> {
        // SAFETY: plain setter on a valid interface; no event context.
        with_endpoint(|v| unsafe { v.SetMasterVolumeLevelScalar(percent.min(100) as f32 / 100.0, std::ptr::null()) })
    }

    pub fn muted() -> Result<bool, String> {
        // SAFETY: plain getter on a valid interface.
        with_endpoint(|v| unsafe { v.GetMute() }).map(|m| m.as_bool())
    }

    pub fn set_mute(mute: bool) -> Result<(), String> {
        // SAFETY: plain setter on a valid interface; no event context.
        with_endpoint(|v| unsafe { v.SetMute(mute, std::ptr::null()) })
    }

    pub fn powercfg(args: &[&str]) -> Result<String, String> {
        use std::os::windows::process::CommandExt;
        let output = Command::new("powercfg.exe")
            .args(args)
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("could not run powercfg: {e}"))?;
        // powercfg writes in the console's code page; the parts we need are plain ASCII.
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() { Ok(text) } else { Err(format!("powercfg refused: {}", text.trim())) }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::super::mac::applescript;

    pub fn dark_mode() -> Result<bool, String> {
        applescript("tell application \"System Events\" to tell appearance preferences to return dark mode").map(|s| s.trim() == "true")
    }
    pub fn set_dark_mode(on: bool) -> Result<(), String> {
        applescript(&format!("tell application \"System Events\" to tell appearance preferences to set dark mode to {on}")).map(|_| ())
    }
    pub fn volume() -> Result<u32, String> {
        applescript("output volume of (get volume settings)").and_then(|s| s.trim().parse().map_err(|_| "unknown volume".to_string()))
    }
    pub fn set_volume(percent: u32) -> Result<(), String> {
        applescript(&format!("set volume output volume {}", percent.min(100))).map(|_| ())
    }
    pub fn muted() -> Result<bool, String> {
        applescript("output muted of (get volume settings)").map(|s| s.trim() == "true")
    }
    pub fn set_mute(mute: bool) -> Result<(), String> {
        applescript(&format!("set volume output muted {mute}")).map(|_| ())
    }
}

const NOT_HERE: &str = "only available on Windows";

fn unsupported<T>() -> Result<T, String> {
    Err(NOT_HERE.into())
}

// Each setting per operating system. Windows reads and writes the same places the Settings app uses.

fn dark_mode() -> Result<bool, String> {
    #[cfg(windows)]
    return Ok(win::read_dword(win::PERSONALIZE, "AppsUseLightTheme") == Some(0));
    #[cfg(target_os = "macos")]
    return mac::dark_mode();
    #[allow(unreachable_code)]
    unsupported()
}

fn set_dark_mode(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let light = u32::from(!on);
        win::write_dword(win::PERSONALIZE, "AppsUseLightTheme", light)?;
        win::write_dword(win::PERSONALIZE, "SystemUsesLightTheme", light)?;
        win::broadcast("ImmersiveColorSet");
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    return mac::set_dark_mode(on);
    #[allow(unreachable_code)]
    {
        let _ = on;
        unsupported()
    }
}

fn transparency() -> Result<bool, String> {
    #[cfg(windows)]
    return Ok(win::read_dword(win::PERSONALIZE, "EnableTransparency") != Some(0));
    #[allow(unreachable_code)]
    unsupported()
}

fn set_transparency(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::write_dword(win::PERSONALIZE, "EnableTransparency", u32::from(on))?;
        win::broadcast("ImmersiveColorSet");
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = on;
        unsupported()
    }
}

fn accent_color() -> Result<String, String> {
    #[cfg(windows)]
    {
        // AccentColor is stored as 0xAABBGGRR.
        let value = win::read_dword(win::DWM, "AccentColor").ok_or("no accent colour is set")?;
        return Ok(format_color(((value & 0xff) as u8, (value >> 8 & 0xff) as u8, (value >> 16 & 0xff) as u8)));
    }
    #[allow(unreachable_code)]
    unsupported()
}

fn set_accent_color(color: &str) -> Result<(), String> {
    let (r, g, b) = parse_color(color)?;
    #[cfg(windows)]
    {
        let abgr = 0xff00_0000 | u32::from(b) << 16 | u32::from(g) << 8 | u32::from(r);
        let argb = 0xc400_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
        // Turn off "pick an accent colour from my background", then set the colour.
        win::write_dword(r"Control Panel\Desktop", "AutoColorization", 0).ok();
        win::write_dword(win::DWM, "AccentColor", abgr)?;
        win::write_dword(win::DWM, "ColorizationColor", argb)?;
        win::write_dword(win::DWM, "ColorizationAfterglow", argb)?;
        win::broadcast("ImmersiveColorSet");
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (r, g, b);
        unsupported()
    }
}

fn volume() -> Result<u32, String> {
    #[cfg(windows)]
    return win::volume();
    #[cfg(target_os = "macos")]
    return mac::volume();
    #[allow(unreachable_code)]
    unsupported()
}

fn set_volume(percent: u32) -> Result<(), String> {
    #[cfg(windows)]
    return win::set_volume(percent);
    #[cfg(target_os = "macos")]
    return mac::set_volume(percent);
    #[allow(unreachable_code)]
    {
        let _ = percent;
        unsupported()
    }
}

fn muted() -> Result<bool, String> {
    #[cfg(windows)]
    return win::muted();
    #[cfg(target_os = "macos")]
    return mac::muted();
    #[allow(unreachable_code)]
    unsupported()
}

fn set_mute(mute: bool) -> Result<(), String> {
    #[cfg(windows)]
    return win::set_mute(mute);
    #[cfg(target_os = "macos")]
    return mac::set_mute(mute);
    #[allow(unreachable_code)]
    {
        let _ = mute;
        unsupported()
    }
}

fn explorer_flag(name: &str, shown_when: u32) -> Result<bool, String> {
    #[cfg(windows)]
    return Ok(win::read_dword(win::EXPLORER_ADVANCED, name).unwrap_or(if shown_when == 1 { 2 } else { 1 }) == shown_when);
    #[allow(unreachable_code)]
    {
        let _ = (name, shown_when);
        unsupported()
    }
}

fn set_explorer_flag(name: &str, value: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::write_dword(win::EXPLORER_ADVANCED, name, value)?;
        win::broadcast("ShellState");
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (name, value);
        unsupported()
    }
}

fn power_plans() -> Result<Vec<(String, String, bool)>, String> {
    #[cfg(windows)]
    return Ok(parse_power_plans(&win::powercfg(&["/list"])?));
    #[allow(unreachable_code)]
    unsupported()
}

fn set_power_plan(wanted: &str) -> Result<String, String> {
    let wanted = wanted.trim().to_lowercase();
    let plans = power_plans()?;
    let plan = plans
        .iter()
        .find(|(name, id, _)| name.to_lowercase() == wanted || *id == wanted)
        .or_else(|| plans.iter().find(|(name, _, _)| name.to_lowercase().contains(&wanted)))
        .ok_or_else(|| {
            let names: Vec<&str> = plans.iter().map(|(n, _, _)| n.as_str()).collect();
            format!("no power plan called \"{wanted}\"; this PC has: {}", names.join(", "))
        })?;
    #[cfg(windows)]
    win::powercfg(&["/setactive", &plan.1])?;
    Ok(plan.0.clone())
}

/// `setting` is "monitor-timeout" or "standby-timeout".
fn set_power_timeout(setting: &str, minutes: u32, when: Option<&str>) -> Result<(), String> {
    let targets: &[&str] = match when {
        Some("plugged") | Some("ac") => &["ac"],
        Some("battery") | Some("dc") => &["dc"],
        None | Some("both") => &["ac", "dc"],
        Some(other) => return Err(format!("unknown \"{other}\": use \"plugged\", \"battery\" or leave it out for both")),
    };
    for target in targets {
        #[cfg(windows)]
        win::powercfg(&["/change", &format!("{setting}-{target}"), &minutes.min(1440).to_string()])?;
        #[cfg(not(windows))]
        {
            let _ = (setting, minutes, target);
            return unsupported();
        }
    }
    Ok(())
}

fn mouse_speed() -> Result<u32, String> {
    #[cfg(windows)]
    return Ok(win::mouse_speed());
    #[allow(unreachable_code)]
    unsupported()
}

fn set_mouse_speed(speed: u32) -> Result<(), String> {
    if !(1..=20).contains(&speed) {
        return Err("the speed goes from 1 (slow) to 20 (fast); Windows' default is 10".into());
    }
    #[cfg(windows)]
    return win::set_mouse_speed(speed);
    #[allow(unreachable_code)]
    unsupported()
}

fn wallpaper() -> Result<String, String> {
    #[cfg(windows)]
    return Ok(win::wallpaper());
    #[allow(unreachable_code)]
    unsupported()
}

// ---- Lua ----------------------------------------------------------------------------

fn getter<T: mlua::IntoLua + 'static>(lua: &Lua, table: &Table, name: &'static str, full: &'static str, f: fn() -> Result<T, String>) -> mlua::Result<()> {
    table.set(name, lua.create_function(move |_, ()| f().map_err(|e| err(full, e)))?)
}

pub fn register(lua: &Lua, allowed: bool, policy: Arc<PathPolicy>) -> mlua::Result<()> {
    let globals = lua.globals();

    let desktop = lua.create_table()?;
    getter(lua, &desktop, "dark_mode", "desktop.dark_mode", dark_mode)?;
    getter(lua, &desktop, "transparency", "desktop.transparency", transparency)?;
    getter(lua, &desktop, "accent_color", "desktop.accent_color", accent_color)?;
    getter(lua, &desktop, "wallpaper", "desktop.wallpaper", wallpaper)?;
    desktop.set(
        "set_dark_mode",
        lua.create_function(move |_, on: bool| {
            require(allowed, "desktop.set_dark_mode")?;
            set_dark_mode(on).map_err(|e| err("desktop.set_dark_mode", e))
        })?,
    )?;
    desktop.set(
        "set_transparency",
        lua.create_function(move |_, on: bool| {
            require(allowed, "desktop.set_transparency")?;
            set_transparency(on).map_err(|e| err("desktop.set_transparency", e))
        })?,
    )?;
    desktop.set(
        "set_accent_color",
        lua.create_function(move |_, color: String| {
            require(allowed, "desktop.set_accent_color")?;
            set_accent_color(&color).map_err(|e| err("desktop.set_accent_color", e))
        })?,
    )?;
    desktop.set(
        "set_wallpaper",
        lua.create_function(move |_, (path, style): (String, Option<String>)| {
            require(allowed, "desktop.set_wallpaper")?;
            let resolved = policy.resolve(&path).map_err(|e| err("desktop.set_wallpaper", e))?;
            if !resolved.is_file() {
                return Err(err("desktop.set_wallpaper", format!("file not found: {path}")));
            }
            #[cfg(windows)]
            return win::set_wallpaper(&resolved, &style.unwrap_or_else(|| "fill".into()).to_lowercase())
                .map_err(|e| err("desktop.set_wallpaper", e));
            #[allow(unreachable_code)]
            {
                let _ = style;
                Err(err("desktop.set_wallpaper", "use system.set_wallpaper on this computer"))
            }
        })?,
    )?;
    globals.set("desktop", desktop)?;

    // Exact volume and mute, next to system.volume_up / volume_down / mute.
    let system: Table = globals.get("system")?;
    getter(lua, &system, "volume", "system.volume", volume)?;
    getter(lua, &system, "muted", "system.muted", muted)?;
    system.set(
        "set_volume",
        lua.create_function(move |_, percent: f64| {
            require(allowed, "system.set_volume")?;
            set_volume(percent.clamp(0.0, 100.0).round() as u32).map_err(|e| err("system.set_volume", e))
        })?,
    )?;
    system.set(
        "set_mute",
        lua.create_function(move |_, mute: bool| {
            require(allowed, "system.set_mute")?;
            set_mute(mute).map_err(|e| err("system.set_mute", e))
        })?,
    )?;

    let power = lua.create_table()?;
    power.set(
        "plans",
        lua.create_function(|lua, ()| {
            let list = lua.create_table()?;
            for (name, id, active) in power_plans().map_err(|e| err("power.plans", e))? {
                let plan = lua.create_table()?;
                plan.set("name", name)?;
                plan.set("id", id)?;
                plan.set("active", active)?;
                list.push(plan)?;
            }
            Ok(list)
        })?,
    )?;
    power.set(
        "plan",
        lua.create_function(|_, ()| {
            let plans = power_plans().map_err(|e| err("power.plan", e))?;
            Ok(plans.into_iter().find(|(_, _, active)| *active).map(|(name, _, _)| name))
        })?,
    )?;
    power.set(
        "set_plan",
        lua.create_function(move |_, name: String| {
            require(allowed, "power.set_plan")?;
            set_power_plan(&name).map_err(|e| err("power.set_plan", e))
        })?,
    )?;
    for (name, setting) in [("set_screen_off", "monitor-timeout"), ("set_sleep", "standby-timeout")] {
        let full: &'static str = if name == "set_sleep" { "power.set_sleep" } else { "power.set_screen_off" };
        power.set(
            name,
            lua.create_function(move |_, (minutes, when): (f64, Option<String>)| {
                require(allowed, full)?;
                set_power_timeout(setting, minutes.max(0.0).round() as u32, when.as_deref()).map_err(|e| err(full, e))
            })?,
        )?;
    }
    globals.set("power", power)?;

    // Mouse speed, next to mouse.move / mouse.click.
    let mouse: Table = globals.get("mouse")?;
    getter(lua, &mouse, "speed", "mouse.speed", mouse_speed)?;
    mouse.set(
        "set_speed",
        lua.create_function(move |_, speed: f64| {
            require(allowed, "mouse.set_speed")?;
            set_mouse_speed(speed.round() as u32).map_err(|e| err("mouse.set_speed", e))
        })?,
    )?;

    let explorer = lua.create_table()?;
    explorer.set("hidden_files", lua.create_function(|_, ()| explorer_flag("Hidden", 1).map_err(|e| err("explorer.hidden_files", e)))?)?;
    explorer.set("file_extensions", lua.create_function(|_, ()| explorer_flag("HideFileExt", 0).map_err(|e| err("explorer.file_extensions", e)))?)?;
    explorer.set(
        "set_hidden_files",
        lua.create_function(move |_, show: bool| {
            require(allowed, "explorer.set_hidden_files")?;
            set_explorer_flag("Hidden", if show { 1 } else { 2 }).map_err(|e| err("explorer.set_hidden_files", e))
        })?,
    )?;
    explorer.set(
        "set_file_extensions",
        lua.create_function(move |_, show: bool| {
            require(allowed, "explorer.set_file_extensions")?;
            set_explorer_flag("HideFileExt", u32::from(!show)).map_err(|e| err("explorer.set_file_extensions", e))
        })?,
    )?;
    globals.set("explorer", explorer)?;
    let _ = Value::Nil;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        assert_eq!(parse_color("#0078D4").unwrap(), (0, 0x78, 0xd4));
        assert_eq!(format_color((0, 0x78, 0xd4)), "#0078d4");
        assert!(parse_color("blue").is_err());
    }

    #[test]
    fn power_plans_are_read_in_any_language() {
        let text = "\nСхемы управления питанием\n-----------------------------------\n\
            GUID схемы питания: 381b4222-f694-41f0-9685-ff5bb260df2e  (Сбалансированная) *\n\
            GUID схемы питания: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Высокая производительность)\n\
            GUID схемы питания: 11111111-2222-3333-4444-555555555555  (My plan)\n";
        let plans = parse_power_plans(text);
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0], ("balanced".into(), "381b4222-f694-41f0-9685-ff5bb260df2e".into(), true));
        assert_eq!(plans[1].0, "high performance");
        assert_eq!(plans[2], ("My plan".into(), "11111111-2222-3333-4444-555555555555".into(), false));
    }
}
