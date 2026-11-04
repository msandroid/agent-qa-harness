---
name: qa-resume-report
description: Resume an interrupted Agent QA inspection, reconcile uncertain actions, and finalize local QA reports with explicit coverage gaps.
---

# Resume and report

Locate the job with `qa_list_jobs`; confirm build and scope with `qa_get_job`. Read the checkpoint and paginate `qa_get_events`. Do not reconstruct missing observations from memory. Restore/reset the target using existing tools; persistent ledger state does not restore browsers, game memory or device sessions.

For each unfinished action, observe the target and use `qa_reconcile_step` to record uncertainty without replaying side effects. Resume paused jobs through `qa_set_status`; wall-clock budget includes pause duration. If budgets are exhausted, save a checkpoint and finalize existing evidence with explicit gaps instead of creating a new job to hide exhaustion.

Use `qa_complete_job` with one outcome for every criterion, then `qa_export_report`. Every pass/fail needs artifact references. An untested or blocked criterion is not a pass. Export before finalization when a progress report is requested, labeling it unfinished. Confirmed issues remain attached to the original build.

Report build/environment, confirmed and intermittent findings, reproduction counts, criteria outcomes, untested areas and evidence locations. Cost is explicitly reported cost, not automatically measured host subscription billing. Reports stay local; publishing or sending them requires the user's instruction.
