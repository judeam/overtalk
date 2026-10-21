# Fresh confirmation cases

These six synthetic text conversations were authored and sealed after the
first 18-case suite exposed a repeat-generation failure. They were frozen before
the greedy-sampling candidate processed them. They are not independent real
recordings or a random population sample.

They cover two cases at each difficulty, across library operations, procurement,
a resilience exercise, an English/Hindi workshop, localization, and research
handover. References include explicit conditions, unavailable names, distinction
between row counts and people, no approval versus inability, and task-specific
deadlines. Expected answers must never enter inference requests.

Freeze new input-only copies with:

```sh
python3 -m evaluations.meetingbench.summary_challenge prepare \
  --corpus evaluations/meetingbench/summary_confirmation --output /tmp/fresh-confirmation
```

Use the same native runner and free-only comparison gateway as the original
challenge. Direct review and scoring accept `--corpus` for this directory. Keep
the original 18-case results separate: after they guide more tuning, they are
regression cases, not a new blind test.
