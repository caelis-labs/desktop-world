//! AT-SPI semantics for a single, independently identified X11 window.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use zbus::{
    address::Transport,
    blocking::{connection::Builder, Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
    Address,
};

const ROOT: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ACTION: &str = "org.a11y.atspi.Action";
const TEXT: &str = "org.a11y.atspi.Text";
const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const MAX_NODES: usize = 256;
const MAX_TEXT_CHARS: i32 = 64 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_millis(250);

// Stable numeric values from the AT-SPI D-Bus protocol. Role names and Action
// descriptions may be localized by the toolkit.
const FRAME: u32 = 23;
const DIALOG: u32 = 16;
const WINDOW: u32 = 69;
const BUTTON: u32 = 43;
const CHECK_BOX: u32 = 7;
const RADIO_BUTTON: u32 = 44;
const TOGGLE_BUTTON: u32 = 62;
const LABEL: u32 = 29;
const TEXT_ROLE: u32 = 61;
const ENTRY: u32 = 79;
const PARAGRAPH: u32 = 73;
const DOCUMENT_TEXT: u32 = 94;
const PASSWORD_TEXT: u32 = 40;
const DEFUNCT: u32 = 6;
const EDITABLE: u32 = 7;
const ENABLED: u32 = 8;
const FOCUSED: u32 = 12;
const SENSITIVE: u32 = 24;
const SHOWING: u32 = 25;
const STALE: u32 = 27;
const VISIBLE: u32 = 30;

type Node = (String, OwnedObjectPath);

#[derive(PartialEq, Eq)]
struct Metadata {
    role: u32,
    name: String,
    enabled: bool,
    editable: bool,
    text: bool,
    focused: bool,
    press: bool,
}

impl Metadata {
    fn role_name(&self) -> &'static str {
        match self.role {
            BUTTON | TOGGLE_BUTTON => "button",
            CHECK_BOX => "checkbox",
            RADIO_BUTTON => "radio_button",
            LABEL | PARAGRAPH => "static_text",
            TEXT_ROLE | ENTRY if self.editable => "text_field",
            TEXT_ROLE | ENTRY | DOCUMENT_TEXT => "text",
            PASSWORD_TEXT => "password_field",
            _ => "unknown",
        }
    }

    fn actions(&self) -> Vec<&'static str> {
        [(self.press, "press"), (self.editable, "setValue")]
            .into_iter()
            .filter_map(|(allowed, action)| (self.enabled && allowed).then_some(action))
            .collect()
    }
}

struct Observed {
    node: Node,
    metadata: Metadata,
    revision: Option<[u8; 32]>,
}

pub(super) struct Query {
    target: WindowTarget,
    connection: Connection,
    root: Node,
    expires: Instant,
    elements: Vec<Observed>,
}

fn connect(address: &str) -> Result<Connection, Error> {
    let address: Address = address.parse().map_err(failed)?;
    if !matches!(address.transport(), Transport::Unix(_)) {
        return Err(Error::UnsupportedCapability);
    }
    Builder::address(address)
        .map_err(failed)?
        .method_timeout(CALL_TIMEOUT)
        .build()
        .map_err(failed)
}

fn bus() -> Result<Connection, Error> {
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| Error::UnsupportedCapability)?;
    let session = connect(&address).map_err(|_| Error::UnsupportedCapability)?;
    let address: String = Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
        .and_then(|proxy| proxy.call("GetAddress", &()))
        .map_err(|_| Error::UnsupportedCapability)?;
    connect(&address).map_err(|_| Error::UnsupportedCapability)
}

fn proxy<'a>(
    connection: &'a Connection,
    node: &'a Node,
    interface: &'static str,
) -> Result<Proxy<'a>, Error> {
    Proxy::new(connection, node.0.as_str(), node.1.as_str(), interface).map_err(failed)
}

fn property(
    connection: &Connection,
    node: &Node,
    interface: &str,
    key: &str,
) -> Result<OwnedValue, Error> {
    proxy(connection, node, "org.freedesktop.DBus.Properties")?
        .call("Get", &(interface, key))
        .map_err(failed)
}

fn children(connection: &Connection, node: &Node) -> Result<Vec<Node>, Error> {
    proxy(connection, node, ACCESSIBLE)?
        .call("GetChildren", &())
        .map_err(failed)
}

fn name(connection: &Connection, node: &Node) -> Result<String, Error> {
    let value =
        String::try_from(property(connection, node, ACCESSIBLE, "Name")?).map_err(failed)?;
    if value.len() > 4096 {
        return Err(failed("AT-SPI name exceeds inspection budget"));
    }
    Ok(value)
}

fn state(connection: &Connection, node: &Node) -> Result<u32, Error> {
    let states: Vec<u32> = proxy(connection, node, ACCESSIBLE)?
        .call("GetState", &())
        .map_err(failed)?;
    match states.as_slice() {
        [flags, _] if !has(*flags, DEFUNCT) && !has(*flags, STALE) => Ok(*flags),
        _ => Err(Error::StaleObservation),
    }
}

fn has(flags: u32, bit: u32) -> bool {
    flags & (1 << bit) != 0
}

fn single_press(connection: &Connection, node: &Node) -> Result<bool, Error> {
    let count = i32::try_from(property(connection, node, ACTION, "NActions")?).map_err(failed)?;
    if count != 1 {
        return Ok(false);
    }
    let action: String = proxy(connection, node, ACTION)?
        .call("GetName", &(0i32,))
        .map_err(failed)?;
    Ok(matches!(action.as_str(), "click" | "press"))
}

fn metadata(connection: &Connection, node: &Node) -> Result<Option<Metadata>, Error> {
    let flags = state(connection, node)?;
    if !has(flags, SHOWING) || !has(flags, VISIBLE) {
        return Ok(None);
    }
    let object = proxy(connection, node, ACCESSIBLE)?;
    let role: u32 = object.call("GetRole", &()).map_err(failed)?;
    let interfaces: Vec<String> = object.call("GetInterfaces", &()).map_err(failed)?;
    let has_interface = |interface: &str| interfaces.iter().any(|candidate| candidate == interface);
    let text = matches!(role, LABEL | TEXT_ROLE | ENTRY | PARAGRAPH | DOCUMENT_TEXT)
        && has_interface(TEXT);
    let enabled = has(flags, ENABLED) && has(flags, SENSITIVE);
    let editable = text
        && matches!(role, TEXT_ROLE | ENTRY)
        && has(flags, EDITABLE)
        && has_interface(EDITABLE_TEXT);
    let press = enabled
        && matches!(role, BUTTON | CHECK_BOX | RADIO_BUTTON | TOGGLE_BUTTON)
        && has_interface(ACTION)
        && single_press(connection, node)?;
    Ok(Some(Metadata {
        role,
        name: name(connection, node)?,
        enabled,
        editable,
        text,
        focused: has(flags, FOCUSED),
        press,
    }))
}

fn text(connection: &Connection, node: &Node) -> Result<String, Error> {
    let count =
        i32::try_from(property(connection, node, TEXT, "CharacterCount")?).map_err(failed)?;
    if !(0..=MAX_TEXT_CHARS).contains(&count) {
        return Err(Error::UnsupportedCapability);
    }
    let text: String = proxy(connection, node, TEXT)?
        .call("GetText", &(0i32, count))
        .map_err(failed)?;
    if text.chars().count() != count as usize {
        return Err(Error::StaleObservation);
    }
    Ok(text)
}

fn is_descendant(
    connection: &Connection,
    node: &Node,
    root: &Node,
    until: Instant,
) -> Result<(), Error> {
    let mut cursor = node.clone();
    for _ in 0..32 {
        check(until)?;
        if cursor == *root {
            return Ok(());
        }
        if cursor.0 != root.0 {
            break;
        }
        cursor =
            Node::try_from(property(connection, &cursor, ACCESSIBLE, "Parent")?).map_err(failed)?;
    }
    Err(Error::StaleObservation)
}

impl Store {
    fn atspi_root(
        &mut self,
        connection: &Connection,
        target: &WindowTarget,
        until: Instant,
    ) -> Result<Node, Error> {
        let window = self.validate(target)?;
        let clients = self.clients()?;
        let owned = inventory::owned_windows(clients, target.app.pid, |window| {
            check(until)?;
            self.owner(window)
        })?;
        if owned != [window]
            || self
                .conn
                .get_window_attributes(window)
                .map_err(failed)?
                .reply()
                .map_err(failed)?
                .map_state
                != MapState::VIEWABLE
        {
            return Err(Error::UnsupportedCapability);
        }
        let registry = (
            "org.a11y.atspi.Registry".into(),
            OwnedObjectPath::try_from(ROOT).map_err(failed)?,
        );
        let apps = children(connection, &registry).map_err(|_| Error::UnsupportedCapability)?;
        if apps.len() > MAX_NODES {
            return Err(Error::UnsupportedCapability);
        }
        let dbus = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(failed)?;
        let mut matching = None;
        for app in apps {
            check(until)?;
            if !app.0.starts_with(':') {
                continue;
            }
            let pid: zbus::Result<u32> =
                dbus.call("GetConnectionUnixProcessID", &(app.0.as_str(),));
            if pid.ok() == Some(target.app.pid) && matching.replace(app).is_some() {
                return Err(Error::UnsupportedCapability);
            }
        }
        let app = matching.ok_or(Error::UnsupportedCapability)?;
        let mut roots = children(connection, &app)?;
        if roots.len() != 1 || roots[0].0 != app.0 {
            return Err(Error::UnsupportedCapability);
        }
        let root = roots.remove(0);
        let role: u32 = proxy(connection, &root, ACCESSIBLE)?
            .call("GetRole", &())
            .map_err(failed)?;
        let flags = state(connection, &root)?;
        if !matches!(role, FRAME | DIALOG | WINDOW) || !has(flags, SHOWING) || !has(flags, VISIBLE)
        {
            return Err(Error::UnsupportedCapability);
        }
        let mut title = self.property(window, "_NET_WM_NAME")?;
        if title.value.is_empty() {
            title = self.property(window, "WM_NAME")?;
        }
        let title = std::str::from_utf8(&title.value).map_err(|_| Error::UnsupportedCapability)?;
        if title.is_empty() || title != name(connection, &root)? {
            return Err(Error::UnsupportedCapability);
        }
        let extents: (i32, i32, i32, i32) = proxy(connection, &root, "org.a11y.atspi.Component")?
            .call("GetExtents", &(0u32,))
            .map_err(|_| Error::UnsupportedCapability)?;
        let bounds_match = |bounds: Bounds| {
            extents
                == (
                    bounds.x.into(),
                    bounds.y.into(),
                    bounds.w.into(),
                    bounds.h.into(),
                )
        };
        if !bounds_match(self.bounds(window)?) && !bounds_match(self.bounds(self.frame(window)?)?) {
            return Err(Error::UnsupportedCapability);
        }
        self.validate(target)?;
        check(until)?;
        Ok(root)
    }

    pub(super) fn observe_atspi(
        &mut self,
        request: &Value,
        until: Instant,
    ) -> Result<Value, Error> {
        let target = parse_target(&request["target"])?;
        let query = &request["query"];
        if query["scope"]["kind"] != "window" {
            return Err(Error::UnsupportedCapability);
        }
        let limit = usize::try_from(number(query, "limit")?)
            .map_err(failed)?
            .clamp(1, 128);
        let needle = query["text"].as_str().map(str::to_lowercase);
        let role = query["role"].as_str();
        let connection = bus()?;
        let root = self.atspi_root(&connection, &target, until)?;
        let mut queue = VecDeque::from(children(&connection, &root)?);
        let mut visited = HashSet::new();
        let mut elements = Vec::new();
        let mut rows = Vec::new();
        let mut gap = None;
        while let Some(node) = queue.pop_front() {
            check(until)?;
            if visited.len() == MAX_NODES || queue.len() >= MAX_NODES {
                gap = Some("node_limit");
                break;
            }
            if node.0 != root.0 || !visited.insert(node.clone()) {
                gap = Some("unproven_descendant");
                continue;
            }
            let (data, children) = match metadata(&connection, &node)
                .and_then(|data| children(&connection, &node).map(|children| (data, children)))
            {
                Ok(result) => result,
                Err(_) => {
                    gap = Some("native_read_gap");
                    continue;
                }
            };
            queue.extend(children);
            let Some(data) = data else { continue };
            let role_name = data.role_name();
            let label = if data.name.is_empty() {
                role_name.to_owned()
            } else {
                format!("{role_name} {}", data.name)
            };
            if role.is_some_and(|role| !role.eq_ignore_ascii_case(role_name))
                || needle.as_ref().is_some_and(|needle| {
                    !crate::backends::text_page::matches_fields(&label, &data.name, "", needle)
                })
            {
                continue;
            }
            if rows.len() == limit {
                gap = Some("element_limit");
                break;
            }
            rows.push(json!({"id":elements.len() as u64 + 1,"label":label,"role":role_name,
                "name":data.name,"enabled":data.enabled,"focused":data.focused,"actions":data.actions()}));
            elements.push(Observed {
                node,
                metadata: data,
                revision: None,
            });
        }
        if self.atspi_root(&connection, &target, until)? != root {
            return Err(Error::TargetChanged);
        }
        // A new semantic query replaces only this window's prior semantic binding.
        self.semantics.retain(|_, query| query.target != target);
        if self.semantics.len() >= 32 {
            return Err(failed("AT-SPI binding budget exhausted"));
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.semantics.insert(
            token.clone(),
            Query {
                target: target.clone(),
                connection,
                root,
                expires: Instant::now() + Duration::from_secs(15),
                elements,
            },
        );
        Ok(
            json!({"target":target_json(&target),"binding":token,"elements":rows,
            "discovery":{"has_more":false,"complete":gap.is_none(),"reason":gap,"visited":visited.len()}}),
        )
    }

    fn verify_element(
        &mut self,
        target: &WindowTarget,
        query: &Query,
        element: usize,
        until: Instant,
    ) -> Result<Metadata, Error> {
        if *target != query.target || query.expires <= Instant::now() {
            return Err(Error::StaleObservation);
        }
        let observed = query.elements.get(element).ok_or(Error::ElementNotFound)?;
        if self.atspi_root(&query.connection, target, until)? != query.root {
            return Err(Error::StaleObservation);
        }
        is_descendant(&query.connection, &observed.node, &query.root, until)
            .map_err(|_| Error::StaleObservation)?;
        let current = metadata(&query.connection, &observed.node)
            .map_err(|_| Error::StaleObservation)?
            .ok_or(Error::StaleObservation)?;
        if current.role != observed.metadata.role || current.name != observed.metadata.name {
            return Err(Error::StaleObservation);
        }
        Ok(current)
    }

    pub(super) fn read_atspi_text(
        &mut self,
        request: &Value,
        until: Instant,
    ) -> Result<Value, Error> {
        let target = parse_target(&request["target"])?;
        let token = string(request, "binding")?.to_owned();
        let element = element_index(request)?;
        let mut query = self
            .semantics
            .remove(&token)
            .ok_or(Error::StaleObservation)?;
        let result = (|| {
            let current = self.verify_element(&target, &query, element, until)?;
            if !current.text {
                return Err(Error::UnsupportedCapability);
            }
            let value = text(&query.connection, &query.elements[element].node)?;
            if !self.verify_element(&target, &query, element, until)?.text {
                return Err(Error::StaleObservation);
            }
            let observed = &mut query.elements[element];
            let revision: [u8; 32] = Sha256::digest(value.as_bytes()).into();
            if observed
                .revision
                .is_some_and(|previous| previous != revision)
            {
                return Err(Error::StaleObservation);
            }
            observed.revision = Some(revision);
            let offset = u32::try_from(number(request, "offset")?).map_err(failed)?;
            let limit = u32::try_from(number(request, "limit")?).map_err(failed)?;
            let page = crate::backends::text_page::page(&value, offset, limit)?;
            Ok(
                json!({"target":target_json(&target),"text":page.text,"offset":page.offset,
                "next_offset":page.next_offset,"total_length":page.total_length,"unit":"utf16"}),
            )
        })();
        if result.is_ok()
            || matches!(&result, Err(Error::UnsupportedCapability))
            || matches!(&result, Err(Error::NativeFailure { code, .. }) if code == "invalid_text_range")
        {
            self.semantics.insert(token, query);
        }
        result
    }

    pub(super) fn action_atspi(&mut self, request: &Value, until: Instant) -> Value {
        let mut sent = false;
        let result = (|| {
            let target = parse_target(&request["target"])?;
            let element = element_index(request)?;
            let query = self
                .semantics
                .remove(string(request, "binding")?)
                .ok_or(Error::StaleObservation)?;
            let operation = string(request, "operation")?;
            let _lease = Lease::acquire(until)?;
            let current = self.verify_element(&target, &query, element, until)?;
            let node = &query.elements[element].node;
            if !current.actions().contains(&operation)
                || !query.elements[element]
                    .metadata
                    .actions()
                    .contains(&operation)
            {
                return Err(Error::UnsupportedCapability);
            }
            let _mutation = worker::begin(until)?;
            let (accepted, requested_value) = if operation == "press" {
                let proxy = proxy(&query.connection, node, ACTION)?;
                sent = true;
                (
                    proxy
                        .call::<_, _, bool>("DoAction", &(0i32,))
                        .map_err(failed)?,
                    None,
                )
            } else {
                let value = string(request, "value")?;
                if !input_sequence::valid_text(value, true) {
                    return Err(Error::InvalidObservation);
                }
                let proxy = proxy(&query.connection, node, EDITABLE_TEXT)?;
                sent = true;
                (
                    proxy
                        .call::<_, _, bool>("SetTextContents", &(value,))
                        .map_err(failed)?,
                    Some(value),
                )
            };
            drop(_mutation);
            if !accepted {
                return Err(failed("AT-SPI did not acknowledge the requested action"));
            }
            let mut reply = json!({"dispatch":"dispatched","evidence":"unverified"});
            if let Some(value) = requested_value {
                let readback = self
                    .verify_element(&target, &query, element, until)
                    .and_then(|current| {
                        if current.text {
                            text(&query.connection, node)
                        } else {
                            Err(Error::UnsupportedCapability)
                        }
                    });
                if readback.as_deref() == Ok(value) {
                    reply["postcondition"] = json!("element_value_set");
                }
            }
            Ok(reply)
        })();
        result.unwrap_or_else(|error| {
            let mut reply = error_reply(error, true);
            if sent {
                reply["dispatch"] = json!("unknown");
            }
            reply
        })
    }

    pub(super) fn retire_atspi(&mut self) {
        self.semantics
            .retain(|_, query| query.expires > Instant::now());
    }
}

fn element_index(request: &Value) -> Result<usize, Error> {
    let id = number(request, "element")?
        .checked_sub(1)
        .ok_or(Error::ElementNotFound)?;
    usize::try_from(id).map_err(|_| Error::ElementNotFound)
}
