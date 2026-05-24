---
name: centralized-logic-check
description: >
  Check whether a calculation, transformation, or piece of business logic is
  properly centralized in a single reusable location rather than duplicated
  across the codebase. Use when implementing or reviewing any computation,
  data transformation, metric calculation, file operation, string
  manipulation, date math, slug normalization, word counting, frontmatter
  parsing, or reusable utility to ensure it follows DRY principles and has a
  single source of truth.
---

# Centralized Logic Check

Before implementing or approving any calculation, transformation, or reusable
operation, verify centralization.

## The Check

Ask yourself and confirm:

> I want to make sure that we do this in a proper, reusable, structured and
> clean way. So this type of calculation should be ideally living in one single
> place, right? And it's reused and referenced everywhere in the code. So we
> don't have 25 different ends that calculate this in different potentially
> ways leading to tech debt down the road. Are we doing that?

## How to Answer

1. **Search for existing implementations** — Look for functions, methods, or
   modules that already perform this calculation or a similar one.
2. **Check imports and references** — See if the codebase already imports or
   calls a centralized version.
3. **Review nearby files** — Check if duplicate logic exists in sibling modules
   or components.
4. **Check DRY catalogs** — If the project has an AGENTS.md, README, or
   `docs/single-source-of-truth-consolidation.md` with a reusable-function
   catalog, consult it.

## If Yes (Already Centralized)

- Confirm the existing function or module name.
- Use it. Do not reimplement.
- Add a cross-reference comment if the connection is non-obvious.

## If No (Not Centralized)

- **Create** a single canonical implementation in the appropriate utility or
  module layer.
- **Move** the logic there if it currently lives inline.
- **Replace** all existing inline copies with calls to the new centralized
  function.
- **Name** it clearly and document its contract (input, output, edge cases).
- **Test** the centralized version and update or remove duplicate tests.

## If Unsure

State the uncertainty explicitly. Do not proceed with duplication. Either:

- Refactor first, then continue, or
- Flag it for follow-up with a `TODO` or `FIXME` comment linking to a planned
  consolidation task.

## Exceptions

Centralization is NOT required when:

- The logic is trivial and unlikely to change (e.g., `a + b` in a single
  location).
- The duplication is intentional for performance or isolation reasons
  (document why).
- The language or framework enforces local definitions (e.g., template helpers,
  React hooks that must live in component files).
