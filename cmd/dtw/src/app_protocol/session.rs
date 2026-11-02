//! Read-only access to the original session ledger, independent of UI effects.
use super::wire::WireError;
use serde_json::{json, Value};

pub(super) struct ReceiptQuery {
    pub execution_id: Option<String>,
    offset: usize,
    limit: usize,
}
fn invalid(message: &str) -> WireError {
    WireError {
        code: "invalid_arguments".into(),
        message: message.into(),
        details: None,
    }
}
pub(super) fn receipt_query(args: &Value) -> Result<ReceiptQuery, WireError> {
    let fields = args.as_object().ok_or_else(|| {
        invalid("session.result accepts an optional query object or execution ID")
    })?;
    if fields
        .keys()
        .any(|key| !["execution_id", "offset", "limit"].contains(&key.as_str()))
    {
        return Err(invalid(
            "session.result accepts only execution_id, offset and limit",
        ));
    }
    let execution_id = fields
        .get("execution_id")
        .map(|id| {
            id.as_str()
                .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| invalid("execution_id must be a returned execution ID"))
        })
        .transpose()?;
    let integer = |key: &str, default: usize, min: usize, max: usize| {
        fields
            .get(key)
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| invalid("Invalid receipt page bounds"))
            })
            .unwrap_or(Ok(default))
    };
    Ok(ReceiptQuery {
        execution_id,
        offset: integer("offset", 0, 0, usize::MAX)?,
        limit: integer("limit", 16, 1, 64)?,
    })
}
pub(super) fn page_receipt(data: &mut Value, query: ReceiptQuery) -> Result<(), WireError> {
    let Some(script) = data.get_mut("script").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let Some(steps) = script.get_mut("steps").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    if query.offset > steps.len() {
        return Err(invalid("Receipt offset exceeds the retained step count"));
    }
    let total = steps.len();
    let mut bytes = 0;
    let mut page = Vec::new();
    for step in steps.iter().skip(query.offset).take(query.limit) {
        let size = step.to_string().len();
        if bytes + size > 256 * 1024 {
            break;
        }
        bytes += size;
        page.push(step.clone());
    }
    if page.is_empty() && query.offset < total {
        return Err(WireError {
            code: "receipt_page_limit".into(),
            message: "A retained step exceeds the script receipt page budget".into(),
            details: None,
        });
    }
    let next = query.offset + page.len();
    *steps = page;
    script.insert("total_steps".into(), json!(total));
    script.insert("offset".into(), json!(query.offset));
    script.insert(
        "next_offset".into(),
        if next < total {
            json!(next)
        } else {
            Value::Null
        },
    );
    Ok(())
}

impl<B: crate::computer::Backend> super::Coordinator<B> {
    pub(super) fn session_metadata(&self) -> Value {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .map(|session| json!({"id":session.id,"app":session.app}))
            .unwrap_or(json!({"id":null,"app":null}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_pages_reject_invalid_bounds_and_never_silently_drop_an_oversized_step() {
        for query in [
            json!({"offset":-1}),
            json!({"limit":0}),
            json!({"limit":65}),
            json!({"offset":1.5}),
            json!({"execution_id":""}),
            json!({"app":"other"}),
        ] {
            assert!(receipt_query(&query).is_err(), "{query}");
        }
        let source = json!({"error":{"code":"original"},"script":{"steps":[
            {"number":1,"details":"x".repeat(150*1024)},
            {"number":2,"details":"x".repeat(150*1024)}]}});
        let mut page = source.clone();
        page_receipt(&mut page, receipt_query(&json!({})).unwrap()).unwrap();
        assert_eq!(page["script"]["total_steps"], 2);
        assert_eq!(page["script"]["next_offset"], 1);
        assert_eq!(page["error"], source["error"]);
        assert_eq!(source["script"]["steps"].as_array().unwrap().len(), 2);
        let mut huge = json!({"script":{"steps":[{"details":"x".repeat(256*1024)}]}});
        assert_eq!(
            page_receipt(&mut huge, receipt_query(&json!({})).unwrap())
                .unwrap_err()
                .code,
            "receipt_page_limit"
        );
        assert!(page_receipt(&mut page, receipt_query(&json!({"offset":3})).unwrap()).is_err());
    }
}
