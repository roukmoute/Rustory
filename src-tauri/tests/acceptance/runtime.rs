//! Gherkin-IR replay engine for the generated acceptance entry points.

use std::collections::{BTreeMap, BTreeSet};

use crate::handlers::{self, SharedState};

/// Execution context of one scenario run.
pub struct World {
    /// Example cell values of the current run (column header -> value).
    pub example: BTreeMap<String, String>,
    /// Example names consumed by the handlers of the current step.
    pub consumed: BTreeSet<String>,
    /// State shared across the steps of the scenario.
    pub state: SharedState,
}

impl World {
    /// Consume one example cell by name. Panics when the scenario provides
    /// no such cell or when a step consumes the same cell twice.
    pub fn require_example(&mut self, name: &str) -> String {
        let value = self.example.get(name).unwrap_or_else(|| {
            panic!("step consumed `<{name}>` but the scenario provides no such example cell")
        });
        if !self.consumed.insert(name.to_owned()) {
            panic!("step consumed `<{name}>` more than once");
        }
        value.clone()
    }
}

struct Step {
    text: String,
}

struct Scenario {
    name: String,
    steps: Vec<Step>,
    examples: Vec<BTreeMap<String, String>>,
}

/// Load the IR: `RUSTORY_ACCEPTANCE_IR` (absolute path written by the APS
/// mutator) when present, else the committed full-feature default.
fn load_scenarios() -> Vec<Scenario> {
    let (source, raw) = if let Some(path) = std::env::var_os("RUSTORY_ACCEPTANCE_IR") {
        let path = path.to_string_lossy().into_owned();
        let source = format!("RUSTORY_ACCEPTANCE_IR={path}");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{source} unreadable: {err}"));
        (source, raw)
    } else {
        // `cargo test` runs the binary from the `src-tauri` package root.
        let source = "tests/acceptance/ir/base.json".to_string();
        let raw = std::fs::read_to_string(&source)
            .unwrap_or_else(|err| panic!("default acceptance IR unreadable: {err}"));
        (source, raw)
    };
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("{source} is not valid JSON: {err}"));
    value
        .get("scenarios")
        .and_then(|scenarios| scenarios.as_array())
        .unwrap_or_else(|| panic!("{source} carries no scenarios"))
        .iter()
        .map(|scenario| {
            let steps = scenario
                .get("steps")
                .and_then(|steps| steps.as_array())
                .map(|steps| {
                    steps
                        .iter()
                        .map(|step| Step {
                            text: step
                                .get("text")
                                .and_then(|text| text.as_str())
                                .unwrap_or("")
                                .to_owned(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let examples = scenario
                .get("examples")
                .and_then(|examples| examples.as_array())
                .map(|examples| {
                    examples
                        .iter()
                        .map(|example| {
                            example
                                .as_object()
                                .unwrap_or_else(|| panic!("example row is not an object: {example}"))
                                .iter()
                                .map(|(key, value)| {
                                    (
                                        key.clone(),
                                        value.as_str().unwrap_or("").to_owned(),
                                    )
                                })
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default();
            Scenario {
                name: scenario
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap_or("<unnamed scenario>")
                    .to_owned(),
                steps,
                examples,
            }
        })
        .collect()
}

/// The `<name>` placeholders declared in a step text (ASCII alphabet, same
/// convention as the APS parser).
fn declared_placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'<' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                let name = &text[start..end];
                if !name.is_empty()
                    && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    found.insert(name.to_owned());
                }
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    found
}

/// Replay one scenario execution.
///
/// `scenario_index` is 1-based over the IR scenarios; `example_index` is 0
/// for scenarios without an Examples table, otherwise 1-based over its
/// rows. Every step must consume exactly the placeholders its text
/// declares — a mutated example cell that a handler silently ignored
/// would surface here as an imbalance.
pub fn run_execution(scenario_index: usize, example_index: usize) {
    let scenarios = load_scenarios();
    let scenario = scenarios
        .get(scenario_index.checked_sub(1).expect("scenario index must be >= 1"))
        .unwrap_or_else(|| {
            panic!("scenario index {scenario_index} out of range (IR has {})", scenarios.len())
        });
    let example: BTreeMap<String, String> = if example_index == 0 {
        if !scenario.examples.is_empty() {
            panic!(
                "scenario `{}` has {} example row(s); example index 0 is reserved for scenarios without examples",
                scenario.name,
                scenario.examples.len()
            );
        }
        BTreeMap::new()
    } else {
        scenario
            .examples
            .get(example_index.checked_sub(1).expect("example index must be >= 1"))
            .unwrap_or_else(|| {
                panic!(
                    "example index {example_index} out of range (scenario `{}` has {})",
                    scenario.name,
                    scenario.examples.len()
                )
            })
            .clone()
    };
    let mut world = World {
        example,
        consumed: BTreeSet::new(),
        state: SharedState::default(),
    };
    for step in &scenario.steps {
        world.consumed.clear();
        handlers::handle(&mut world, &scenario.name, &step.text);
        let declared = declared_placeholders(&step.text);
        if declared != world.consumed {
            panic!(
                "placeholder imbalance in step `{}` of scenario `{}`: declared {:?}, consumed {:?}",
                step.text, scenario.name, declared, world.consumed
            );
        }
    }
}
