use crate::computer::{Error, ForegroundFrame};
use serde_json::{json, Value};

pub(crate) fn foreground_json(frame: ForegroundFrame) -> Value {
    json!({"bounds":frame.bounds,"dpi":frame.dpi,"render_size":frame.render_size,
        "source":frame.source,"size":frame.size})
}

pub(crate) fn foreground_frame(value: &Value) -> Result<ForegroundFrame, Error> {
    let invalid = |_| Error::InvalidObservation;
    let frame = ForegroundFrame {
        bounds: serde_json::from_value(value["bounds"].clone()).map_err(invalid)?,
        dpi: serde_json::from_value(value["dpi"].clone()).map_err(invalid)?,
        render_size: serde_json::from_value(value["render_size"].clone()).map_err(invalid)?,
        source: serde_json::from_value(value["source"].clone()).map_err(invalid)?,
        size: serde_json::from_value(value["size"].clone()).map_err(invalid)?,
    };
    if frame.dpi == 0 || frame.render_size.iter().any(|&size| size <= 0) {
        return Err(Error::InvalidObservation);
    }
    for axis in 0..2 {
        let origin = i64::from(frame.bounds[axis]);
        let start = i64::from(frame.source[axis]);
        let extent = i64::from(frame.source[axis + 2]);
        if start < origin
            || extent <= 0
            || start + extent > origin + i64::from(frame.bounds[axis + 2])
            || frame.size[axis] == 0
            || frame.size[axis] > 1600
        {
            return Err(Error::InvalidObservation);
        }
    }
    Ok(frame)
}
