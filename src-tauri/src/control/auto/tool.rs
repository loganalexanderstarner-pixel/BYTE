//! What a job stage may use besides the PC and the browser: tools other parts of BYTE plug in (connectors that message people,
//! for example). A tool is a small trait; nothing here depends on any connector. The runner checks the contract (who may be
//! messaged, the message budget) before it calls a tool.

use std::sync::{Arc, Mutex, OnceLock};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::contract::{Contract, Fact, Meter, Verdict};

/// Something a tool sent: to whom, and a short note of what (never the whole text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    pub to: String,
    pub what: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutcome {
    pub ok: bool,
    /// One or two sentences for the report.
    pub summary: String,
    /// Titles or addresses of what was read (not the content).
    pub read: Vec<String>,
    pub sent: Vec<Sent>,
}

pub trait JobTool: Send + Sync {
    /// The name a stage uses for it ("discord", "telegram").
    fn name(&self) -> &str;
    /// What it does and the JSON arguments it takes, in a sentence or two, for the model that fills them in.
    fn describe(&self) -> String;
    /// Who a call would message (checked against the contract before the call).
    fn recipients(&self, _args: &Value) -> Vec<String> {
        vec![]
    }
    fn run<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, ToolOutcome>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Mutex<Vec<Arc<dyn JobTool>>>,
}

impl ToolRegistry {
    pub fn register(&self, t: Arc<dyn JobTool>) {
        if let Ok(mut v) = self.tools.lock() {
            v.retain(|x| x.name() != t.name());
            v.push(t);
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn JobTool>> {
        self.tools.lock().ok()?.iter().find(|t| t.name() == name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.lock().map(|v| v.iter().map(|t| t.name().to_string()).collect()).unwrap_or_default()
    }

    pub fn descriptions(&self) -> Vec<(String, String)> {
        self.tools.lock().map(|v| v.iter().map(|t| (t.name().to_string(), t.describe())).collect()).unwrap_or_default()
    }
}

static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();

/// The tools plugged in so far. Other parts of BYTE call `registry().register(...)` at startup.
pub fn registry() -> &'static ToolRegistry {
    REGISTRY.get_or_init(ToolRegistry::default)
}

/// Why a tool call may not go ahead, or None. Spends one message from the budget per recipient (or one when it names none but
/// sends something), only when every recipient is allowed.
pub fn check_call(tool: &dyn JobTool, call: &ToolCall, contract: &Contract, meter: &Meter) -> Result<(), CallBlock> {
    let to = tool.recipients(&call.args);
    for r in &to {
        match contract.judge(&Fact::Message(r.clone()), &[]) {
            Verdict::Inside => {}
            Verdict::Outside(why) => return Err(CallBlock::Outside(why)),
            Verdict::Refused(why) => return Err(CallBlock::Refused(why)),
        }
    }
    for _ in 0..to.len() {
        meter.take_message().map_err(CallBlock::Budget)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallBlock {
    /// Outside the contract: the run asks the person.
    Outside(String),
    Refused(String),
    /// A budget ran out: the run stops.
    Budget(String),
}

#[cfg(test)]
pub struct FakeTool {
    pub name: String,
    pub to: Vec<String>,
    pub calls: Mutex<Vec<Value>>,
}

#[cfg(test)]
impl FakeTool {
    pub fn new(name: &str, to: &[&str]) -> Arc<FakeTool> {
        Arc::new(FakeTool { name: name.into(), to: to.iter().map(|s| s.to_string()).collect(), calls: Mutex::new(vec![]) })
    }
}

#[cfg(test)]
impl JobTool for FakeTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn describe(&self) -> String {
        format!("{} sends a message. Arguments: {{\"to\":\"...\",\"text\":\"...\"}}", self.name)
    }
    fn recipients(&self, args: &Value) -> Vec<String> {
        args.get("to").and_then(Value::as_str).map(|s| vec![s.to_string()]).unwrap_or_else(|| self.to.clone())
    }
    fn run<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, ToolOutcome> {
        self.calls.lock().unwrap().push(call.args.clone());
        let to = self.recipients(&call.args);
        Box::pin(async move { ToolOutcome { ok: true, summary: format!("sent to {}", to.join(", ")), read: vec![], sent: to.into_iter().map(|t| Sent { to: t, what: "a message".into() }).collect() } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn contract() -> Contract {
        Contract { may_message: vec!["mom".into()], ..Default::default() }
    }

    #[test]
    fn the_registry_replaces_a_tool_with_the_same_name() {
        let r = ToolRegistry::default();
        r.register(FakeTool::new("discord", &[]));
        r.register(FakeTool::new("discord", &["x"]));
        r.register(FakeTool::new("telegram", &[]));
        assert_eq!(r.names(), vec!["discord".to_string(), "telegram".to_string()]);
        assert!(r.get("telegram").is_some() && r.get("nope").is_none());
    }

    #[test]
    fn a_call_to_someone_outside_the_contract_waits_for_the_person() {
        let t = FakeTool::new("discord", &[]);
        let m = Meter::new(None, Some(5), 0, 0);
        let call = ToolCall { tool: "discord".into(), args: json!({"to":"boss","text":"hi"}) };
        assert!(matches!(check_call(&*t, &call, &contract(), &m), Err(CallBlock::Outside(_))));
        assert_eq!(m.messages(), 0, "nothing was spent");
    }

    #[test]
    fn an_allowed_call_spends_the_message_budget_until_it_runs_out() {
        let t = FakeTool::new("discord", &[]);
        let m = Meter::new(None, Some(1), 0, 0);
        let call = ToolCall { tool: "discord".into(), args: json!({"to":"mom"}) };
        assert!(check_call(&*t, &call, &contract(), &m).is_ok());
        assert!(matches!(check_call(&*t, &call, &contract(), &m), Err(CallBlock::Budget(_))));
    }

    #[test]
    fn the_double_records_what_it_was_asked() {
        let t = FakeTool::new("discord", &[]);
        let call = ToolCall { tool: "discord".into(), args: json!({"to":"mom","text":"hi"}) };
        let out = futures_util::FutureExt::now_or_never(t.run(&call)).unwrap();
        assert!(out.ok && out.sent[0].to == "mom");
        assert_eq!(t.calls.lock().unwrap().len(), 1);
    }
}
