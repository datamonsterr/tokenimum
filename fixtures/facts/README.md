# Synthetic facts acceptance and TE port map

Reference: microsoft/TokenEconomizer commit `d0675a3385948d330a793863794a432f3490b1ac` (Apache-2.0).

Fixtures are synthetic Rust Session/Call builders in `crates/tkm-core/tests/facts_te.rs` `facts_safety.rs`, and `facts_review.rs`; no user data or reference scripts are executed.

## TE acceptance cases

| TE case | Rust test / applicability |
| --- | --- |
| `tests/test_envfacts.py::test_classify_forms_and_intents` | `te_envfacts_test_classify_forms_and_intents` |
| `tests/test_envfacts.py::test_error_classes_are_environmental_only` | `te_envfacts_test_error_classes_are_environmental_only` |
| `tests/test_envfacts.py::test_interpreter_fact_forms_fires_and_renders` | `te_envfacts_test_interpreter_fact_forms_fires_and_renders` |
| `tests/test_envfacts.py::test_clean_environment_forms_nothing` | `te_envfacts_test_clean_environment_forms_nothing` |
| `tests/test_envfacts.py::test_prevented_waste_still_counts_as_firing` | `te_envfacts_test_prevented_waste_still_counts_as_firing` |
| `tests/test_envfacts.py::test_bare_runner_success_contradicts_and_retires` | `te_envfacts_test_bare_runner_success_contradicts_and_retires` |
| `tests/test_envfacts.py::test_bare_one_liner_success_is_not_a_contradiction` | `te_envfacts_test_bare_one_liner_success_is_not_a_contradiction` |
| `tests/test_envfacts.py::test_history_fact_forms_only_when_probes_return_nothing` | `te_envfacts_test_history_fact_forms_only_when_probes_return_nothing` |
| `tests/test_envfacts.py::test_refused_hosts_form_a_boundary_fact_and_a_reachable_host_does_not` | `te_envfacts_test_refused_hosts_form_a_boundary_fact_and_a_reachable_host_does_not` |
| `tests/test_envfacts.py::test_lint_refuses_completion_advice` | `te_envfacts_test_lint_refuses_completion_advice` |
| `tests/test_envfacts.py::test_block_is_capped_in_facts_and_tokens` | `te_envfacts_test_block_is_capped_in_facts_and_tokens` |
| `tests/test_envfacts.py::test_form_repair_facts_sharing_the_working_form_merge_into_one_line` | `te_envfacts_test_form_repair_facts_sharing_the_working_form_merge_into_one_line` |
| `tests/test_envfacts.py::test_a_merged_line_with_one_working_form_says_works` | `te_envfacts_test_a_merged_line_with_one_working_form_says_works` |
| `tests/test_envfacts.py::test_an_environment_fact_forms_in_a_repository_where_every_session_edits_one_file` | `te_envfacts_test_an_environment_fact_forms_in_a_repository_where_every_session_edits_one_file` |
| `tests/test_envfacts.py::test_a_form_that_only_worked_with_an_assignment_before_it_is_not_named` | `te_envfacts_test_a_form_that_only_worked_with_an_assignment_before_it_is_not_named` |
| `tests/test_envfacts.py::test_one_session_is_one_piece_of_evidence_per_form` | `te_envfacts_test_one_session_is_one_piece_of_evidence_per_form` |
| `tests/test_envfacts.py::test_a_named_form_that_stops_working_is_named_no_more` | `te_envfacts_test_a_named_form_that_stops_working_is_named_no_more` |
| `tests/test_envfacts.py::test_env_assignments_after_env_are_part_of_how_a_command_was_typed` | `te_envfacts_test_env_assignments_after_env_are_part_of_how_a_command_was_typed` |
| `tests/test_envfacts.py::test_save_load_roundtrip` | `te_envfacts_test_save_load_roundtrip` |
| `tests/test_envfacts.py::test_claude_schema_yields_the_same_fact` | Native JSON transcript parsing belongs to E5 readers; core accepts frozen normalized Session/Call values. |
| `tests/test_envfacts.py::test_prefix_stage_envfacts_profile_needs_no_runner_and_reports_zero` | Python PrefixStage wiring and billing rows belong to daemon/ledger integration; core has no model client or I/O. |
| `tests/test_envfacts.py::test_the_classifier_sees_through_rtks_command_prefix` | `te_envfacts_test_the_classifier_sees_through_rtks_command_prefix` |
| `tests/test_envfacts_args.py::test_only_a_shape_rewrite_counts_as_a_repair` | `te_envfacts_test_only_a_shape_rewrite_counts_as_a_repair` |
| `tests/test_envfacts_args.py::test_labels_are_the_structured_arguments_after_the_runner` | `te_envfacts_test_labels_are_the_structured_arguments_after_the_runner` |
| `tests/test_envfacts_args.py::test_a_label_prefix_the_runner_rejects_becomes_a_fact` | `te_envfacts_test_a_label_prefix_the_runner_rejects_becomes_a_fact` |
| `tests/test_envfacts_args.py::test_pytest_style_separators_the_runner_rejects_become_a_fact` | `te_envfacts_test_pytest_style_separators_the_runner_rejects_become_a_fact` |
| `tests/test_envfacts_args.py::test_a_plain_mistake_is_not_a_fact` | `te_envfacts_test_a_plain_mistake_is_not_a_fact` |
| `tests/test_envfacts_args.py::test_a_prefixed_label_that_loads_contradicts_the_fact` | `te_envfacts_test_a_prefixed_label_that_loads_contradicts_the_fact` |
| `tests/test_envfacts_args.py::test_an_environment_variable_the_code_asks_for_and_its_working_value_become_a_fact` | `te_envfacts_test_an_environment_variable_the_code_asks_for_and_its_working_value_become_a_fact` |
| `tests/test_envfacts_args.py::test_a_value_set_in_the_code_counts_too` | `te_envfacts_test_a_value_set_in_the_code_counts_too` |
| `tests/test_envfacts_args.py::test_no_value_that_worked_means_no_fact` | `te_envfacts_test_no_value_that_worked_means_no_fact` |
| `tests/test_envfacts_args.py::test_output_filtered_away_neither_teaches_nor_contradicts` | `te_envfacts_test_output_filtered_away_neither_teaches_nor_contradicts` |
| `tests/test_envfacts_args.py::test_a_repair_needs_the_rewritten_label_to_have_run_tests` | `te_envfacts_test_a_repair_needs_the_rewritten_label_to_have_run_tests` |
| `tests/test_envfacts_args.py::test_being_asked_for_the_variable_again_does_not_confirm_its_value` | `te_envfacts_test_being_asked_for_the_variable_again_does_not_confirm_its_value` |
| `tests/test_envfacts_args.py::test_mistyped_labels_do_not_teach_that_the_runner_fails` | `te_envfacts_test_mistyped_labels_do_not_teach_that_the_runner_fails` |
| `tests/test_envfacts_args.py::test_a_directory_change_is_part_of_the_form` | `te_envfacts_test_a_directory_change_is_part_of_the_form` |
| `tests/test_envfacts_args.py::test_a_missing_runner_is_still_a_form_failure_not_a_label_one` | `te_envfacts_test_a_missing_runner_is_still_a_form_failure_not_a_label_one` |
| `tests/test_envfacts_args.py::test_a_bare_runner_in_a_directory_an_earlier_cd_left_is_learned_with_that_cd` | Per-call cwd is absent from frozen Call. classify_command_at accepts explicit cwd and the pure cwd table covers equivalent forms; reader acquisition/integration is deferred. |
| `tests/test_envfacts_args.py::test_a_cd_into_the_start_directory_by_absolute_path_is_no_cd` | `te_envfacts_test_a_cd_into_the_start_directory_by_absolute_path_is_no_cd` |
| `tests/test_envfacts_args.py::test_a_cd_up_to_the_repository_from_a_subdirectory_is_no_cd` | `te_envfacts_test_a_cd_up_to_the_repository_from_a_subdirectory_is_no_cd` |
| `tests/test_envfacts_args.py::test_the_repository_is_the_nearest_directory_holding_git` | Filesystem .git discovery is forbidden in pure core and belongs to repository identity/readers. Caller supplies root/top. |
| `tests/test_envfacts_args.py::test_sessions_started_in_a_subdirectory_learn_the_repositorys_plain_form` | End-to-end transcript cwd plus .git discovery requires E1/E5 metadata. Pure caller-supplied root/top normalization is covered by the cd-up table. |
| `tests/test_envfacts_args.py::test_a_command_stopped_by_its_own_cd_teaches_nothing` | `te_envfacts_test_a_command_stopped_by_its_own_cd_teaches_nothing` |
| `tests/test_envfacts_args.py::test_call_directories_line_up_with_the_calls` | Native trace reader alignment belongs to E5; frozen model Call has no cwd. Core flattened call-index alignment is covered separately. |
| `tests/test_envfacts_args.py::test_a_runner_script_run_where_it_is_not_becomes_a_fact` | `te_envfacts_test_a_runner_script_run_where_it_is_not_becomes_a_fact` |
| `tests/test_envfacts_args.py::test_an_option_the_runner_rejects_is_not_a_failing_form` | `te_envfacts_test_an_option_the_runner_rejects_is_not_a_failing_form` |
| `tests/test_envfacts_args.py::test_a_run_whose_output_was_filtered_away_does_not_show_a_form_works` | `te_envfacts_test_a_run_whose_output_was_filtered_away_does_not_show_a_form_works` |
| `tests/test_envfacts_args.py::test_a_working_form_that_leaves_the_shell_where_it_is_is_the_one_named` | `te_envfacts_test_a_working_form_that_leaves_the_shell_where_it_is_is_the_one_named` |

## Functions

| TE path / item | Rust item |
| --- | --- |
| `microskills/observe.py::{GENERIC, segments, unwrap, classify_command, _classify, _probe_kind}` | `facts::observe::{GENERIC, segments, classify_command, classify_command_at}` and private helpers |
| `observe.py::{error_class, _exit_code_hint, _test_labels, _label_failed, label_rule, _env_calls, observe_task, form_failed, form_worked, is_bare}` | `facts::observe::{error_class, label_failed, label_rule, observe}` and private attribution/env/form helpers |
| `lifecycle.py::edited_files` | `observe::observe` reads normalized `Call.edits`, all languages; no transcript parsing |
| `envfacts.py::EnvFactsDistiller.update` (five families) | `extract::{extract, exercise_*}`, `facts::update` |
| `envfacts.py::{typed_ok, _repairs, EnvFact.working_form}` | `extract::{typed_ok, repairs, working_form}` |
| `envfacts.py::{fold, CONTRADICT_STREAK}` | `lifecycle::fold`, including two exercised-session form contradiction streak |
| `lifecycle.py::Lifecycle::{update, score, injectable, firing_rate}` | `lifecycle::{Lifecycle, Status, fold}`, `Lifecycle::injectable`; incremental recency score and counters |
| `envfacts.py::{lint_ok, _LINT_RE}` | `lint::{lint_ok, validate}` plus SPEC 13.4 path/regex restrictions |
| `envfacts.py::{EnvFact.claim, _usage, _root, _join, _merged_lines, _select, render}` | `render::{claim, render}` plus private code templates/merging/selection |
| `envfacts.py::{to_dict, from_dict}` | Serde on all state/evidence/config types; roundtrip test |
| `envfacts.py::{seed_injected, injected_ids}` | Caller supplies `SessionEvidence.injected_ids`; prevented-waste test |

## Additional DoD evidence

- E2.3: classification, error precedence, rtk parity, quoted/heredoc segmentation, probe hit tables, generic summaries, all-language edits, conservative outcomes, explicit call indices.
- E2.4: named TE family cases, parameter validation table for all five families, structural label rewrite table, no evidence from silent output, one-session counts and whole-setting rejection.
- E2.5: three-session promotion with externally supplied decisions, four in code-only mode, active fire gate at 40% and four exercises, 0.8 decay, 5.0 contradiction weight, candidate/active high-stakes hard kills, persistent serde/replay protection, host scope.
- Safety: `lint_table_rejects_completion_verification_and_external_paths`, `rendered_text_uses_only_templates_and_validated_parameters`, `parameter_validation_table_for_all_families`, `rejects_whole_invalid_env_values_instead_of_learning_a_safe_prefix`.
- Table-driven checks cover observation, extraction, lifecycle, lint, and rendering boundaries.

## Adaptations and boundaries

- TE absolute interpreter fixtures (`/opt/env/...`) and home paths are replaced by equivalent repository-relative `.venv/...`/`.local/...` paths. Classification can observe absolute forms, but candidate validation/rendering rejects them per SPEC 13.4.
- SPEC 7.1 strengthens TE: failure of the currently recommended working form is an immediate code contradiction, even when a model rates contradiction low. The TE two-exercised-session streak applies to the formerly failing bare runner now succeeding, and does not suppress failure of a promised working form.
- Fetch reachability is stricter than TE for unrelated `echo`, `printf`, or `cat` segments: their output cannot prove a fetch succeeded. The pass-through probe rule is retained.
- Exact template wording is retained from TE. Generic header is used; header, bullets and final newline all count under the four-characters-per-token cap. Relative cd forms render as subshells.
- Environment setting values must match the test-label grammar; arbitrary strings, paths outside the repo and shell expressions are rejected. Errors render through a closed enum, never raw output.
- Shell words retain complete quoted/unquoted concatenations before validation; substitutions (`$(`, backticks, `${`) are opaque and cannot supply values. Escaped operators and quotes do not create command segments; assignment-looking prose does not set a variable. Django `manage.py test` retains its subcommand in the canonical form.
- Working forms are regex-validated before candidate filtering and host scope derivation. Candidate parameters carry complete successful forms with one session of evidence; lifecycle accounting does not count discovery twice.
- Lint rejects file URIs, drive-relative Windows paths, absolute Windows paths and UNC paths while preserving test-label double colons.
- `args_digest` must be normalized shell command text. No JSON decoding is introduced (serde_json remains a dev dependency). Explicit exit_code plus TE digest hints supply outcomes.
- `classify_command_at` accepts cwd relative to a supplied root and optional known repository top. `observe` uses session cwd and explicit cds; it cannot infer persistent per-call cwd from the frozen model.
- Partial sessions and replayed (host, exact agent_home, native session_id) identities do not train facts. Environment facts deliberately do not require different edited files (TE drift=false).
- Config/threshold validation fails open. Decisions are caller-provided calibrated scores; unavailable/invalid threshold batches require the extra code-only confirmation. Explicitly rejected stable/safe verdicts never add confirmations or successful-form counts; code contradictions still count. Contradiction probabilities outside [0,1] or non-finite values are ignored.
- Native transcript schemas, filesystem discovery/persistence, plugin detection, Python experiment vocabulary/proposer logic, model calls, daemon wiring and ledger accounting are outside this pure module. E2.9 owns event explanation prose. Controller owns NOTICE.

## Every Rust test mapped to DoD

| Test | DoD |
| --- | --- |
| `facts_te.rs::te_envfacts_test_classify_forms_and_intents` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_error_classes_are_environmental_only` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_interpreter_fact_forms_fires_and_renders` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::te_envfacts_test_clean_environment_forms_nothing` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_prevented_waste_still_counts_as_firing` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::te_envfacts_test_bare_runner_success_contradicts_and_retires` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::te_envfacts_test_bare_one_liner_success_is_not_a_contradiction` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_history_fact_forms_only_when_probes_return_nothing` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::te_envfacts_test_refused_hosts_form_a_boundary_fact_and_a_reachable_host_does_not` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_lint_refuses_completion_advice` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_te.rs::te_envfacts_test_block_is_capped_in_facts_and_tokens` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_form_repair_facts_sharing_the_working_form_merge_into_one_line` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_merged_line_with_one_working_form_says_works` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_an_environment_fact_forms_in_a_repository_where_every_session_edits_one_file` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_form_that_only_worked_with_an_assignment_before_it_is_not_named` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_one_session_is_one_piece_of_evidence_per_form` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_named_form_that_stops_working_is_named_no_more` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_env_assignments_after_env_are_part_of_how_a_command_was_typed` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_save_load_roundtrip` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::te_envfacts_test_the_classifier_sees_through_rtks_command_prefix` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_only_a_shape_rewrite_counts_as_a_repair` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_labels_are_the_structured_arguments_after_the_runner` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_label_prefix_the_runner_rejects_becomes_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_pytest_style_separators_the_runner_rejects_become_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_plain_mistake_is_not_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_prefixed_label_that_loads_contradicts_the_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_an_environment_variable_the_code_asks_for_and_its_working_value_become_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_value_set_in_the_code_counts_too` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_no_value_that_worked_means_no_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_output_filtered_away_neither_teaches_nor_contradicts` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_repair_needs_the_rewritten_label_to_have_run_tests` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_being_asked_for_the_variable_again_does_not_confirm_its_value` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_mistyped_labels_do_not_teach_that_the_runner_fails` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_directory_change_is_part_of_the_form` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_missing_runner_is_still_a_form_failure_not_a_label_one` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_cd_into_the_start_directory_by_absolute_path_is_no_cd` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_cd_up_to_the_repository_from_a_subdirectory_is_no_cd` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_command_stopped_by_its_own_cd_teaches_nothing` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_envfacts_test_a_runner_script_run_where_it_is_not_becomes_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_an_option_the_runner_rejects_is_not_a_failing_form` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_run_whose_output_was_filtered_away_does_not_show_a_form_works` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_envfacts_test_a_working_form_that_leaves_the_shell_where_it_is_is_the_one_named` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_generic_reading_knows_builds_and_other_ecosystems` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::te_generic_build_tool_repair_becomes_a_fact` | E2.4 five-family extraction / TE behavioural parity |
| `facts_te.rs::te_injection_empty_render_before_formation_and_grounded_firing` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::decision_thresholds_code_only_and_events` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::lint_table_rejects_completion_verification_and_external_paths` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_te.rs::rendered_text_uses_only_templates_and_validated_parameters` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_te.rs::parameter_validation_table_for_all_families` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_te.rs::lifecycle_table_firing_gate_and_decay` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::partial_replay_and_evidence_indices` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::host_scope_and_neutral_scope_rendering` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::conservative_attribution_and_reported_exit_codes` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::segments_table_and_heredoc` | E2.3 observation / table-driven TE parity |
| `facts_te.rs::high_stakes_candidate_hard_kill_and_decision_contradiction` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_te.rs::defaults_match_spec_and_zero_caps_fail_open` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_safety.rs::rejects_whole_invalid_env_values_instead_of_learning_a_safe_prefix` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_safety.rs::quoted_env_values_remain_literal_and_validated` | E2.5 lint / template-only injection; E2.4 regex validation |
| `facts_safety.rs::observation_error_precedence_table` | E2.3 observation / table-driven TE parity |
| `facts_safety.rs::probe_kinds_and_hit_patterns_table` | E2.3 observation / table-driven TE parity |
| `facts_safety.rs::generic_test_summaries_and_all_edited_languages` | E2.3 observation / table-driven TE parity |
| `facts_safety.rs::scope_is_inferred_from_host_tool_names` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_safety.rs::invalid_thresholds_use_code_only_and_invalid_config_abstains` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_safety.rs::boundary_success_with_failure_in_same_session_never_promotes` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_safety.rs::code_contradictions_apply_configured_recency_weight_despite_low_model_score` | E2.5 lifecycle / decisions / firing / scope / persistence |
| `facts_safety.rs::failure_of_the_recommended_form_is_always_a_code_contradiction` | E2.5 SPEC code contradiction precedence / retirement |
| `facts_review.rs::review_rejected_scores_never_confirm_an_existing_candidate` | Review 1: rejection does not become a code-only confirmation or successful-form count |
| `facts_review.rs::review_rejected_filter_scores_still_allow_code_contradictions` | Review 1: rejecting filter cannot erase code contradictions |
| `facts_review.rs::review_segments_respect_unquoted_escapes_and_shell_quote_rules` | Review 2: escaped operators and shell quoting preserve segment boundaries |
| `facts_review.rs::review_assignment_parser_preserves_complete_concatenated_words` | Review 3: complete assignment words are parsed before validation |
| `facts_review.rs::review_assignment_substitutions_and_quoted_prose_cannot_teach_values` | Review 3: substitutions and quoted prose do not teach env values |
| `facts_review.rs::review_working_forms_are_validated_and_scoped_before_candidate_filtering` | Review 4: complete working forms are validated and scoped before decisions |
| `facts_review.rs::review_lint_and_command_validation_reject_file_uris_and_windows_paths` | Review 5: file URIs and Windows/UNC path restrictions in lint and command grammar |
| `facts_review.rs::review_django_manage_test_forms_are_canonical_and_renderable` | Review 6: Django test subcommand remains canonical and renderable |
| `facts_review.rs::review_replay_identity_includes_host_and_exact_agent_home` | Review 7: host/account-scoped replay protection and serde roundtrip |
| `facts_review.rs::review_malformed_contradiction_scores_are_ignored` | Review 8: malformed contradiction scores are ignored |

## API documentation checked

Checked 2026-10-29: [regex 1.13.1](https://docs.rs/regex/1.13.1/regex/struct.Regex.html), [Rust 1.99.1 str](https://doc.rust-lang.org/std/primitive.str.html), [Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html), [BTreeMap](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html), [BTreeSet](https://doc.rust-lang.org/std/collections/struct.BTreeSet.html), [f64](https://doc.rust-lang.org/std/primitive.f64.html), [Option](https://doc.rust-lang.org/std/option/enum.Option.html), [Iterator](https://doc.rust-lang.org/std/iter/trait.Iterator.html), [Path](https://doc.rust-lang.org/std/path/struct.Path.html), [serde 1.0.229](https://docs.rs/serde/1.0.229/serde/), and [serde_json 1.0.154](https://docs.rs/serde_json/1.0.154/serde_json/).
