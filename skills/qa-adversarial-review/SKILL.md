---
name: qa-adversarial-review
description: Find executable counterexamples to product behavior, specifications or software changes and record them as evidence-backed QA findings.
---

# Seek counterexamples

Use the same Agent QA contract, evidence and reproduction workflow as exploratory QA. Define the guarantee being challenged and the authorized target. Review does not itself authorize changes to production systems, external accounts or unrelated targets.

Choose relevant hypotheses: interruption during persistence, duplicate/reordered operations, stale UI versus backend state, role changes, invalid boundaries, concurrent clients, reconnect/restore, or mismatch between documented and observed behavior. Favor a small executable counterexample over a long list of speculative complaints.

Use existing application tools, tests or source inspection to test each hypothesis. Record source reasoning as an observation or note; establish a runtime defect with execution evidence when feasible. If only static evidence is available, report that limitation explicitly and leave runtime confirmation pending.

Create candidate findings with expected/actual results and artifacts. Reproduce via the `qa-reproduce` workflow when possible. Do not claim exhaustive coverage or absence of vulnerabilities. Preserve counterevidence, false hypotheses and untested areas in the checkpoint and final report.
