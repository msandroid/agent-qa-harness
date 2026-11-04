---
name: qa-reproduce
description: Reproduce a recorded Agent QA finding from a reset fixture, collect independent run evidence and distinguish confirmed, intermittent and unreproduced issues.
---

# Verify a finding

Read the job contract and candidate. Preserve the original build. Reset with existing target tools and create a separate `reproduction` run using `qa_begin_run`. Close any existing harness browser before opening a fresh context for this run. Describe what was actually reset; starting a run does not execute a reset.

Execute the smallest useful version of the reported steps. Collect fresh evidence under this run and call `qa_record_reproduction` once per attempt. Never reuse discovery artifacts as reproduction proof. For another attempt, reset and create another run. Mixed successes/failures remain intermittent; an unsuccessful attempt does not erase original evidence.

If the cause is unclear, distinguish the observed symptom from hypotheses, product failure, automation failure and missing specification. Neither repeated model agreement nor a plausible explanation substitutes for executing the reproduction. Use a separate read context/model when it helps reduce anchoring, without assuming extra agents or paid APIs are available.

Do not fix the product as part of reproduction unless the user's task includes implementation. A changed build needs a new job for regression testing; this job's build contract is immutable. Export results with reproduction counts and limitations.
