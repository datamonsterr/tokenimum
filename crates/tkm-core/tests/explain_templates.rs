#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use tkm_core::inject::explain::{
    ActionKind, ActionParams, AdapterDisabledParams, BudgetReachedParams, DenyListProposalParams,
    DenyListWriteParams, EscalationParams, Explanation, FactInjectedParams, FactPromotedParams,
    FactRetiredParams, Params, RedirectParams, RestoreParams, render,
};
fn explanation(kind: ActionKind) -> Explanation {
    let p = Params {
        event: "Event 1".into(),
        detail: "detail".into(),
        why: "reason".into(),
        model_checks: "checks".into(),
        cost: "$0".into(),
        undo: "undo".into(),
    };
    let params = match kind {
        ActionKind::FactInjected => ActionParams::FactInjected(FactInjectedParams(p)),
        ActionKind::FactPromoted => ActionParams::FactPromoted(FactPromotedParams(p)),
        ActionKind::FactRetired => ActionParams::FactRetired(FactRetiredParams(p)),
        ActionKind::Redirect => ActionParams::Redirect(RedirectParams(p)),
        ActionKind::Escalation => ActionParams::Escalation(EscalationParams(p)),
        ActionKind::DenyListProposal => ActionParams::DenyListProposal(DenyListProposalParams(p)),
        ActionKind::DenyListWrite => ActionParams::DenyListWrite(DenyListWriteParams(p)),
        ActionKind::Restore => ActionParams::Restore(RestoreParams(p)),
        ActionKind::BudgetReached => ActionParams::BudgetReached(BudgetReachedParams(p)),
        ActionKind::AdapterDisabled => ActionParams::AdapterDisabled(AdapterDisabledParams(p)),
    };
    Explanation {
        template_id: kind,
        params,
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
    if let ActionParams::Redirect(ref mut p) = e.params {
        p.0.detail = "line\nforged".into();
    }
    assert!(!render(&e).contains("line\nforged"));
}
#[test]
fn explain_spec_example() {
    let e = Explanation { template_id: ActionKind::FactInjected, params: ActionParams::FactInjected(FactInjectedParams(Params {
        event: "Event 4812, 2026-11-03 14:22, Claude Code, repo github.com/acme/api".into(),
        detail: "Run tests with `python -m pytest` (`pytest` fails here: ImportError).".into(),
        why: "in 4 of the last 5 sessions in this repository, `pytest` failed with an ImportError and `python -m pytest` then worked. The fact was confirmed in 3 sessions (needed: 3) and never contradicted.".into(),
        model_checks: "inception/mercury-decide-20260930 rated it stable (0.94) and safe (0.97).".into(),
        cost: "$0.000006".into(), undo: "tkm facts forget f-17".into(),
    })) };
    assert_eq!(
        render(&e),
        "Event 4812, 2026-11-03 14:22, Claude Code, repo github.com/acme/api\n\nWhat happened: tokenimum added a short note to the agent's context:\n  \"Run tests with `python -m pytest` (`pytest` fails here: ImportError).\"\n\nWhy: in 4 of the last 5 sessions in this repository, `pytest` failed with an\nImportError and `python -m pytest` then worked. The fact was confirmed in 3\nsessions (needed: 3) and never contradicted.\n\nModel checks: inception/mercury-decide-20260930 rated it stable (0.94) and safe (0.97).\nCost of those checks: $0.000006.\n\nUndo: tkm facts forget f-17"
    );
}
#[test]
fn mismatched_template_and_parameters_are_rejected() {
    let mut explanation = explanation(ActionKind::Redirect);
    explanation.template_id = ActionKind::Restore;
    assert_eq!(render(&explanation), "");
}
