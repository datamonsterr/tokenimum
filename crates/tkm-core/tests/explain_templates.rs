use tkm_core::inject::explain::{ActionKind, Explanation, Params, render};
fn explanation(kind: ActionKind) -> Explanation {
    Explanation {
        template_id: kind,
        params: Params {
            event: "Event 1".into(),
            detail: "detail".into(),
            why: "reason".into(),
            model_checks: "checks".into(),
            cost: "$0".into(),
            undo: "undo".into(),
        },
    }
}
macro_rules! render_test {
    ($name:ident, $kind:ident) => {
        #[test]
        fn $name() {
            let e = explanation(ActionKind::$kind);
            let text = render(&e);
            for section in ["What happened:", "Why:", "Model checks:", "Cost", "Undo:"] {
                assert!(text.contains(section), "{section}");
            }
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(serde_json::from_str::<Explanation>(&json).unwrap(), e);
        }
    };
}
render_test!(explain_renders_fact_injected, FactInjected);
render_test!(explain_renders_fact_promoted, FactPromoted);
render_test!(explain_renders_fact_retired, FactRetired);
render_test!(explain_renders_redirect, Redirect);
render_test!(explain_renders_escalation, Escalation);
render_test!(explain_renders_deny_list_proposal, DenyListProposal);
render_test!(explain_renders_deny_list_write, DenyListWrite);
render_test!(explain_renders_restore, Restore);
render_test!(explain_renders_budget_reached, BudgetReached);
render_test!(explain_renders_adapter_disabled, AdapterDisabled);
#[test]
fn explain_sanitizes_parameters() {
    let mut e = explanation(ActionKind::Redirect);
    e.params.detail = "line\nforged".into();
    assert!(!render(&e).contains("line\nforged"));
}
