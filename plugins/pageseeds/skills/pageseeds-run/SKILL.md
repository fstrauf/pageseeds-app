---
name: pageseeds-run
description: >-
  v2 SEO loop for one PageSeeds project: begin a pageseeds-run worktree, write one
  article per create row of the worklist through Path B, merge each merge row through
  merge-context / merge-submit, noindex the prune rows (code-applied at finish), then ship one PR that holds only the files the rows own. Use for /pageseeds-run, "run the SEO loop", "write this
  week's new pages". Operator only; never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/pageseeds-run", "pageseeds run", "run the v2 loop", "create lane".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "run start -> write each create row, merge each merge row -> run finish -> PR"
---

# PageSeeds Run (v2 loop: create and merge lanes)

Canonical path: `.agents/skills/pageseeds-run/SKILL.md`. Rules and the row schema: `docs/RUN.md`.
Lanes: create and merge (agent work), prune (code-applied at `run finish`, no agent step).

## Flow

1. **Start.** `pageseeds-cli run -i <id>`. It opens a `pageseeds-run` worktree, plans inside it
   and prints the worklist, the scoreboard and `next`: one `write-context … --worklist`
   command per create row and one `merge-context` command per merge row. A hard error (stale GSC tape, missing `run.yaml`) means stop and
   report it; nothing was started. Another active session blocks the start: report it, do not
   abort a session you did not open.
2. **For each create row, in rank order:**
   1. Run the row's `write-context -i <id> -p <worktree> -K "<keyword>" --worklist [--source <id>]`.
   2. Write the MDX to `target_file` from `skill_content`, `content_brief`, `facts`, `authors`
      and `evidence`. Numbers, ratings and claims only from `facts`/`evidence`/named sources.
   3. `pageseeds-cli write-submit -i <id> -p <worktree> -f <target_file> -K "<keyword>"`.
      If `ok` is false fix the listed issues and resubmit, at most 2 resubmits, then skip the row.
   4. `pageseeds-cli publish-content -i <id> -p <worktree> -S <slug>`. A `blocked` entry
      (truth review) is final: do not retry, reword or edit around it. Skip the row.
3. **For each merge row, in rank order** (a row is a source page redirected into a keeper, chosen
   by GSC shared-query evidence; `merge_evidence` is `unknown` when the data is missing or stale,
   and then there are no merge rows to work):
   1. `pageseeds-cli merge-context -i <id> -p <worktree> -K /blog/<into_slug> -R /blog/<from_slug>`.
      Its `evidence` carries the same verdict `merge-submit` will reach. If a source's verdict is
      not `merge`, skip the row and report it.
   2. Fold the source's unique, still-true content into the keeper MDX at `keeper_file`
      (tables, FAQs, examples from `redirects[].content`; Read the full file when
      `content_truncated`). Numbers and claims only from the source page, `facts`/`evidence` or
      named sources. Do not invent content and do not change the keeper's slug.
   3. `pageseeds-cli merge-submit -i <id> -p <worktree> -K /blog/<into_slug> -R /blog/<from_slug>`.
   4. On `ok:false` skip the row and report the failed check (`merge_evidence`, `truth_review`,
      `valid_mdx`, ...). Fix and resubmit only for `valid_mdx` or `min_keeper_words`, at most 2
      resubmits. A `merge_evidence` or `truth_review` refusal is final: nothing was applied, so
      do not reword around it.
4. **Prune rows need no work from you.** Each prune row is a dead page the stock-take picked
   (`reason` starts with `noindex`). `run finish` sets `robots: "noindex, follow"` on it in the
   worktree after re-checking that it still earns nothing. Do not edit those MDX files, do not
   noindex pages yourself, and do not redirect or delete a prune page: a page with a same-intent
   earning page is a merge row, not a prune row. Merging the run PR is the approval.
5. **Finish.** `pageseeds-cli run finish -i <id>`. It ships one PR with the published create
   rows, the applied merge rows (keeper, deleted source, redirects CSV, pages whose links were
   rewritten), the noindexed prune pages and `articles.json` only, and prints `outcomes`,
   `collateral` and `ship`.
6. **Report** a table: rank, keyword, `source -> keeper` or prune page, status (`published`,
   `not_published`, `not_written`, `blocked_at_ship`, `merged`, `not_merged`, `pruned`,
   `not_pruned`), file, reason (for `not_pruned` the apply-time refusal, e.g. "page earns N
   clicks"), plus the PR URL (`ship.pr_url`) or `ship.note`/`skip_reason`
   when there is none. Never present a missing PR as a success.

## Scheduled (started by operator-runs)

`/pageseeds-run <id> --started` means the launcher already ran step 1 and will run step 5:
the `pageseeds-run` worktree exists, the worklist is planned, and the prompt carries the worktree
path and the row commands (`next`). Do steps 2 to 4 only, in the worktree. Do NOT call
`pageseeds-cli run` or `pageseeds-cli run finish`, `worktree ship` or `worktree abort`: the
launcher finishes the run after you exit (whatever the exit code), ships the one PR and
records the status. Rows you did not finish are reported as `not_written` and the status is
`warn`, so skip a failing row and move on rather than retrying past the limits above.

## Rails

- Never write a keyword that is not a create row (`write-context --worklist` refuses it).
- Never merge a pair that is not a merge row, and never pick a different keeper: the evidence
  gate refuses a pair it did not choose.
- Never edit files other than a create row's `target_file` and a merge row's `keeper_file`.
- Never noindex a page by hand for a prune row; the run owns it.
- Never call `worktree ship` directly; `run finish` is the only ship.
- A truth block is not retried or edited around; skip the row.
- Abort = `pageseeds-cli worktree abort -i <id>`.
