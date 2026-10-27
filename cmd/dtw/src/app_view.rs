//! Application observation and one-use coordinate bindings. CUA remains unmodified.
use crate::SavedResult;
use cua_driver_sdk::{DriverError, ImageContent};
use serde_json::{json, Value};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
mod macos;
pub(super) fn helper_entry() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::helper_entry()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
async fn platform(request: Value) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(move || macos::call(request))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = request;
        Err("Application observation extension requires macOS".into())
    }
}
#[derive(Default)]
pub(super) struct Runtime {
    observation: Mutex<Option<Observation>>,
}
struct Observation {
    id: String,
    pid: u32,
    created: Instant,
    inventory: Value,
    frames: Vec<Value>,
}
impl Runtime {
    pub fn invalidate(&self) {
        *self.observation.lock().unwrap() = None;
    }
}
fn refusal(code: &str, message: &str) -> SavedResult {
    SavedResult {
        native_operation: None,
        text: message.into(),
        structured: json!({"code":code,"coverage":"incomplete"}).to_string(),
        images: vec![],
        is_error: true,
        error_code: Some(code.into()),
    }
}
fn platform_error(value: &Value) -> Option<SavedResult> {
    value["error"]
        .as_str()
        .map(|code| refusal(code, value["message"].as_str().unwrap_or(code)))
}
fn inventory_equal(a: &Value, b: &Value) -> bool {
    if a["process_identity"] != b["process_identity"] {
        return false;
    }
    let normalize = |v: &Value| {
        v["windows"].as_array().map(|ws| ws.iter().map(|w| json!({"pid":w["pid"],"id":w["id"],"bounds":w["bounds"],"layer":w["layer"]})).collect::<Vec<_>>())
    };
    normalize(a).is_some() && normalize(a) == normalize(b)
}
fn point(frame: &Value, x: f64, y: f64) -> Option<(f64, f64)> {
    let width = frame["width"].as_f64()?;
    let height = frame["height"].as_f64()?;
    let b = frame["bounds"].as_array()?;
    if b.len() != 4
        || !x.is_finite()
        || !y.is_finite()
        || width <= 0.
        || height <= 0.
        || x < 0.
        || y < 0.
        || x >= width
        || y >= height
    {
        return None;
    }
    Some((
        b[0].as_f64()? + x / width * b[2].as_f64()?,
        b[1].as_f64()? + y / height * b[3].as_f64()?,
    ))
}
pub(super) async fn capture(
    runtime: &Runtime,
    pid: u32,
    cap: u32,
    limit: usize,
    window_id: Option<u32>,
) -> Result<SavedResult, DriverError> {
    // WindowServer may advertise a newly opened sheet before SCK can share it.
    // Retry only a whole read-only observation, never an action or a partial
    // image set. Permission, timeout and geometry-contract errors fail directly.
    for attempt in 0..3 {
        let result = capture_once(runtime, pid, cap, limit, window_id).await?;
        let transitioning = matches!(
            result.error_code.as_deref(),
            Some("window_unavailable" | "app_windows_changed")
        );
        if !transitioning || attempt == 2 {
            return Ok(result);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    unreachable!()
}

async fn capture_once(
    runtime: &Runtime,
    pid: u32,
    cap: u32,
    limit: usize,
    window_id: Option<u32>,
) -> Result<SavedResult, DriverError> {
    runtime.invalidate();
    let before = match platform(json!({"operation":"discover","pid":pid})).await {
        Ok(v) => v,
        Err(e) => return Ok(refusal("app_windows_unavailable", &e)),
    };
    if let Some(e) = platform_error(&before) {
        return Ok(e);
    }
    let windows = before["windows"].as_array().cloned().unwrap_or_default();
    if windows.is_empty() {
        return Ok(refusal(
            "app_windows_unavailable",
            "No attributable visible windows; widen observation explicitly if needed",
        ));
    }
    let mut frames = Vec::new();
    let mut images = Vec::new();
    let selected: Vec<_> = windows
        .iter()
        .filter(|w| window_id.is_none_or(|id| w["id"].as_u64() == Some(u64::from(id))))
        .take(if window_id.is_some() { 1 } else { limit })
        .collect();
    if selected.is_empty() {
        return Ok(refusal("window_not_attributed", "Requested window is not an attributable visible candidate; enumerate windows again. No fallback capture."));
    }
    for w in selected {
        // One image/coordinate contract for all attributed surfaces. CUA's
        // outer-dimension validity check does not detect composed sheet images.
        let v=match platform(json!({"operation":"capture","pid":w["pid"],"window_id":w["id"],"max_image_dimension":cap})).await { Ok(v)=>v,Err(e)=>return Ok(refusal("app_capture_incomplete",&e)) };
        if let Some(e) = platform_error(&v) {
            return Ok(e);
        }
        // Reject a window that moved between inventory and the native SCK frame.
        if v["bounds"] != w["bounds"] {
            return Ok(refusal(
                "app_windows_changed",
                "SCK geometry changed during capture; observe again",
            ));
        }
        let Some(png) = v["png"].as_str() else {
            return Ok(refusal("app_capture_incomplete", "No SCK image"));
        };
        frames.push(json!({"image_index":images.len(),"pid":w["pid"],"window_id":w["id"],"bounds":w["bounds"],"layer":w["layer"],"stack_rank":w["rank"],"association":w["association"],"backend":"screencapturekit","input_route":"dtw_visible_point","width":v["width"],"height":v["height"]}));
        images.push(ImageContent {
            mime_type: "image/png".into(),
            data_base64: png.into(),
        });
    }
    let after = match platform(json!({"operation":"discover","pid":pid})).await {
        Ok(v) => v,
        Err(e) => return Ok(refusal("app_windows_changed", &e)),
    };
    if !inventory_equal(&before, &after) {
        return Ok(refusal(
            "app_windows_changed",
            "Associated windows changed during capture; observe again",
        ));
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let selection = if window_id.is_some() {
        "exact_window_id"
    } else {
        "front_to_back"
    };
    let summary = json!({"pid":pid,"observation_id":id,"scope":"app","frames":frames,"windows":windows,"occluders":after["occluders"],"selection":selection,"requested_window_id":window_id,"omitted_count":windows.len().saturating_sub(frames.len()),"association_complete":false,"coverage":"attributed_visible_windows","discovery":"public_os_best_effort","expires_in_seconds":30});
    *runtime.observation.lock().unwrap() = Some(Observation {
        id: id.clone(),
        pid,
        created: Instant::now(),
        inventory: after,
        frames,
    });
    Ok(SavedResult{native_operation:Some("app".into()),text:format!("App {pid}: {} observed window(s), {} image(s), selection={selection}. observation_id={id}. Fetch image_index; use operation=click with observation_id, image_index, x, y. Attribution is best effort; inspect occluders and omitted_count. Reobserve after input.",windows.len(),images.len()),structured:summary.to_string(),images,is_error:false,error_code:None})
}
pub(super) async fn click(
    runtime: &Runtime,
    id: &str,
    index: usize,
    x: f64,
    y: f64,
) -> Result<SavedResult, DriverError> {
    let observation = {
        let mut slot = runtime.observation.lock().unwrap();
        if slot.as_ref().is_none_or(|o| o.id != id) {
            return Ok(refusal(
                "stale_observation",
                "Unknown or consumed observation; observe again",
            ));
        }
        slot.take().unwrap()
    };
    if observation.created.elapsed() > Duration::from_secs(30) {
        return Ok(refusal(
            "stale_observation",
            "Observation expired; observe again",
        ));
    }
    let Some(frame) = observation.frames.get(index) else {
        return Ok(refusal("invalid_point", "Unknown image_index"));
    };
    let Some((sx, sy)) = point(frame, x, y) else {
        return Ok(refusal(
            "invalid_point",
            "Point must be within the original returned PNG",
        ));
    };
    let now = match platform(json!({"operation":"discover","pid":observation.pid})).await {
        Ok(v) => v,
        Err(e) => return Ok(refusal("stale_observation", &e)),
    };
    if !inventory_equal(&observation.inventory, &now) {
        return Ok(refusal(
            "stale_observation",
            "Application windows changed; no input posted",
        ));
    }
    let guard=match platform(json!({"operation":"validate_point","pid":frame["pid"],"window_id":frame["window_id"],"bounds":frame["bounds"],"screen_x":sx,"screen_y":sy})).await {Ok(v)=>v,Err(e)=>return Ok(refusal("input_unavailable",&e))};
    if let Some(e) = platform_error(&guard) {
        return Ok(e);
    }
    let v=match platform(json!({"operation":"click","pid":frame["pid"],"window_id":frame["window_id"],"bounds":frame["bounds"],"screen_x":sx,"screen_y":sy})).await {Ok(v)=>v,Err(e)=>return Ok(refusal("input_outcome_unknown",&e))};
    if let Some(e) = platform_error(&v) {
        return Ok(e);
    }
    Ok(SavedResult{native_operation:None,text:"Pointer input posted to the verified visible window. Effect is unverified; observe again before further input.".into(),structured:v.to_string(),images:vec![],is_error:false,error_code:None})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixel_mapping_handles_scaled_negative_origin_and_rejects_edges() {
        let f = json!({"width":1000,"height":500,"bounds":[-800,20,500,250]});
        assert_eq!(point(&f, 400., 100.), Some((-600., 70.)));
        for (x, y) in [(1000., 1.), (-1., 1.), (1., 500.), (f64::NAN, 0.)] {
            assert_eq!(point(&f, x, y), None);
        }
    }
    #[test]
    fn inventory_checks_identity_geometry_and_order_not_absolute_rank() {
        let a = json!({"process_identity":"a","windows":[{"pid":1,"id":2,"bounds":[0,0,100,100],"layer":0,"rank":1},{"pid":1,"id":3,"bounds":[0,0,100,100],"layer":0,"rank":2}]});
        let mut b = a.clone();
        b["windows"][0]["rank"] = json!(8);
        assert!(inventory_equal(&a, &b));
        b["windows"][0]["bounds"][0] = json!(1);
        assert!(!inventory_equal(&a, &b));
        b = a.clone();
        b["windows"].as_array_mut().unwrap().reverse();
        assert!(!inventory_equal(&a, &b));
        b = a.clone();
        b["process_identity"] = json!("restarted");
        assert!(!inventory_equal(&a, &b));
    }
}
