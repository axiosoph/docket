# Fixture manifest

Each subdirectory of `fixtures/` is a **self-contained corpus root**: its
own `docket.ncl` plus a `docs/` tree, meant to be pointed at as one
invocation (`docket check fixtures/<name>/`). Corpus roots never share
state — nothing in one fixture directory can collide with another,
document identifiers included, since an identifier is a corpus-relative
path and corpus roots don't overlap.

Every failing fixture is minimal in the strict sense stated in the
dispatch: **remove any line from it and it stops isolating the named
check** — either because the fixture now passes outright, or because a
*different* check would start firing instead (both count as "no longer
isolates check X alone"). Where a fixture needs more than the bare
minimum to prevent a second, unrelated check from also firing, that is
called out below.

| fixture | fails | why |
|:---|:---|:---|
| `c1-unknown-field/` | `C1` | `docs/specs/example.md`'s claim block has `extra: nope`, a field the contract does not declare. MVP.md §1.2: "Unknown fields are an error, not ignored." The `[example-claim]` heading is present (not omitted) so the block is *not* also orphaned — omitting it would swap the failure to `orphan-claim` rather than removing it, which is why it stays. |
| `c2-duplicate-claim-id/` | `C2` | `docs/specs/a.md` and `docs/specs/b.md` both head a claim `[dup-id]`. Each claim block is independently well-formed, in a genre that permits its kind, with no `depends`/`because` — so nothing but the id collision fails. |
| `c3-genre-forbids-kind/` | `C3` | `docket.ncl` declares `docs/adr/**` with `kinds = []` (MVP.md §2's "no claims permitted" form). `docs/adr/0001-decision.md` declares a `requirement` claim there anyway. |
| `c4-dangling-cite/` | `C4` | `docs/specs/a.md`'s claim `depends: [nonexistent-claim]`, which resolves to nothing in the corpus. A prose link `[related work](nonexistent-claim)` with the identical target is present so `C5` (every `depends`/`because` entry has a matching prose link) still passes — see the C5-vs-C4 judgment call below. |
| `c5-prose-cites-mismatch/` | `C5` | `docs/specs/a.md` declares `[target-claim]` (cited, and it resolves — so `C4` passes) and `[mismatched-claim]`, which declares `depends: [target-claim]` but whose prose body carries no matching link. `[target-claim]` exists specifically so the citation resolves and `C4` cannot also fire. |
| `orphan-claim/` | `orphan-claim` | `docs/specs/a.md` is a claim block with no preceding heading in the file at all — the minimal case of "no `[id]`-shaped heading precedes it" (MVP.md §1.1). |
| `orphaned-because/` | `orphaned-because` | `docs/specs/a.md`'s claim declares `because: [nonexistent-claim]`, which resolves to nothing. A matching prose link is present so `C5` still passes. Exits **0** — `orphaned-because` is `Warn` severity, distinct from `C4`'s `Fail`; see below. |
| `bare-reference-no-failure/` | — (all pass) | Not a check fixture — see below. `docs/specs/a.md`'s claim declares neither `depends` nor `because`, but its prose links to a target that does not exist in the corpus. Proves the noise-suppression property: an undeclared (bare) reference is never resolved against the corpus at all. |
| `explanation-forbids-kinds/` | — (config error) | Not a `C`-check fixture — see below. Isolates the `quadrant`/`kinds` derived rule (MVP.md §2), a `docket.ncl` config error, not a corpus-content check. |
| `normative-prose-unclaimed/` | `normative-prose` | `docs/adr/0001-decision.md` (genre `docs/adr/**`, `kinds = []`) carries a bare `MUST` in its own prose, with no claim block anywhere in the file. See below. |
| `normative-prose-quoted/` | — (all pass) | Not a check fixture — see below. The identical keyword, present only inside a block quote and inside an inline code span, in the same `kinds = []` genre. |
| `blast-doc-anchor/` | — (all pass) | Not a check fixture — see below. Isolates a `blast` behaviour rather than a check. |
| `golden/` | — (all pass) | See below. |

**`duplicate-stem/` is retired**, not just its row here. MVP.md §1.3 was
amended once a real corpus produced three `README.md` files under one
genre (`docs/models/{lean,lean-surety,tla}/README.md`), indistinguishable
by basename: document identifiers are now corpus-relative paths, unique
by construction, so there is nothing left for a stem-collision check to
detect. The fixture directory and its two empty files are deleted.

**Every fixture's `docket.ncl` carries the required `quadrant` field**
(MVP.md §2), added to every genre without changing which check the
fixture isolates: `docs/specs/**`/`docs/models/**`/`docs/architecture/**`
genres are `quadrant = "reference"`, and `docs/adr/**` (`kinds = []`)
genres are `quadrant = "explanation"` — the same mapping MVP.md §2's own
worked example uses. Neither value interacts with any `C`-check or
`orphan-claim`, so no fixture's isolated failure changed.

## `explanation-forbids-kinds/`

Isolates the derived rule (MVP.md §2): a genre whose `quadrant` is
`explanation` and whose `kinds` is non-empty is a configuration error.
`docket.ncl` declares `docs/adr/**` with `kinds = ["requirement"]` and
`quadrant = "explanation"` — the exact contradiction the rule forbids.

This is **not** one of the five corpus checks (C1–C5) or `orphan-claim`:
it is caught at config-load time, before the corpus is ever scanned, the
same way an ambiguous genre pattern or an unknown `kind` value already
are. `docket check --corpus fixtures/explanation-forbids-kinds` exits
**2** (usage/configuration error, MVP.md §5), with a message naming the
offending genre and its `kinds`, rather than exiting 1 with a `Failure`
entry in the index.

**The rule is enforced only in Rust (`config::load_config`), not in
`contracts/docket.ncl`.** `nickel export --apply-contract
contracts/docket.ncl` on this fixture's `docket.ncl` exits **0** — the
Nickel contract validates the *shape* of `quadrant` and `kinds`
independently but does not cross-check them against each other, by
design (see `contracts/docket.ncl`'s comment on `Genre`). Only
`docket check` (or a direct call to `config::load_config`) observes the
failure. This fixture's row above is marked "config error" rather than
"all pass" for exactly that reason: the corpus content is fine, and would
pass every `C`-check if the config ever let it load.

`docs/adr/0001-decision.md` mirrors `c3-genre-forbids-kind/`'s decision
document — a `requirement` claim under an ADR-shaped path — kept for
realism as a self-contained corpus root, even though `docket check` never
reaches it: the config error is returned before any file is scanned.

## `normative-prose-unclaimed/` and `normative-prose-quoted/`

Isolate the `normative-prose` check: a genre whose `kinds` is empty
permits no claim blocks, which already means no RFC-2119 keyword may
appear in that genre's own-voice text either — the mechanism by which a
decision record is prevented from carrying normative content extends
from *claim blocks* (`kinds = []`, C3) to *unregistered prose* (this
check), since a bare `MUST` is a binding assertion whether or not it
carries a block.

Both fixtures share one genre — `docs/adr/**`, `kinds = []`,
`quadrant = "explanation"` — and neither carries a claim block at all;
the check fires independently of C3/orphan-claim, so nothing else in
either corpus is capable of failing.

- **`normative-prose-unclaimed/`** — `docs/adr/0001-decision.md` line 3:
  `This decision MUST be treated as final.`, a bare own-voice `MUST`.
  `docket check --corpus fixtures/normative-prose-unclaimed` exits **1**
  with exactly one `normative-prose` failure naming the file, the line,
  and the genre.
- **`normative-prose-quoted/`** — the identical keyword, present twice,
  neither instance in the document's own voice: once inside a block
  quote (`> ... MUST retry.`, quoting a rejected proposal so the
  document can refute it) and once inside an inline code span
  (`` `MUST` ``, naming the token rather than asserting it). This is the
  fixture that proves the *design* rather than the feature — a
  regex over raw text cannot tell a quoted `MUST` from an asserted one,
  and this corpus is built to make exactly that distinction load-bearing.
  `docket check --corpus fixtures/normative-prose-quoted` exits **0**.

## `orphaned-because/` and `bare-reference-no-failure/`

Isolate the reference-kinds severity split (MVP.md §1.2, §3): a `depends`
target and a `because` target carry different remedies when dangling, and
a reference declared as neither is not this tool's concern at all.

- **`orphaned-because/`** — `docs/specs/a.md`'s claim
  `[orphaned-because-claim]` declares `because: [nonexistent-claim]`, with
  a matching prose link (`[the old reason](nonexistent-claim)`, so C5
  cannot also fire). `docket check --corpus fixtures/orphaned-because`
  exits **0** — a `Warn`-severity diagnostic never flips the exit code —
  while still printing exactly one `orphaned-because` diagnostic on
  stderr, whose message says the *reason* is orphaned, not the claim.
  This is the fixture that would have exited 1 under a design that gave
  `depends` and `because` the same severity, which is exactly the design
  R1 (the reference-kinds requirements) rejects.
- **`bare-reference-no-failure/`** — `docs/specs/a.md`'s claim
  `[bare-ref-claim]` declares neither `depends` nor `because` at all; its
  prose links to `nonexistent-target`, which resolves to nothing in the
  corpus. `docket check --corpus fixtures/bare-reference-no-failure` exits
  **0** with **zero** diagnostics — not a warning, not a failure. This is
  the noise-suppression property: an undeclared prose link is a bare
  reference by construction (nothing marks it as one; the absence of a
  `depends`/`because` entry *is* the marking), so it is never resolved
  against the corpus, and a target's deletion produces no signal at all.
  Also proves C5's replaced rule is one-directional: an undeclared prose
  link is never required to correspond to a declared entry.

## `blast-doc-anchor/`

Isolates the defect fixed alongside `blast-semantics`: a `depends`/`because`
entry that is a document anchor (§1.3) must create a reverse edge, the
same as a claim-id entry does. Under the pre-fix implementation, this
corpus's blast query returned nothing; a maintainer relying on it would
have missed exactly the class of dependency §4.1's own worked example
depends on.

- `docs/models/rule.md` — hosts `## 3. The rule`, a section with no
  claim block of its own: the fixture isolates a citation *targeting a
  section*, not a claim living in one.
- `docs/specs/a.md` — claim `[depends-on-rule]` declares
  `depends: [docs/models/rule#3]`, the **doc-path#anchor** form, with a
  matching prose link.
- `docs/specs/b.md` — claim `[depends-transitively]` declares
  `depends: [depends-on-rule]` by claim id, with a matching prose link.
  This proves
  the walk does not dead-end at a document-anchor citer: `x` (found via
  the anchor) is still citable by its own claim id, exactly like any
  other claim.

`docket blast 'docs/models/rule#3' --corpus fixtures/blast-doc-anchor`
must report both `depends-on-rule` and `depends-transitively`, in that
order. The corpus passes `docket check` cleanly — this fixture
demonstrates a `blast` behaviour, not a check failure.

## `golden/`

One document per MVP.md §2's example genre (`docs/specs`, `docs/models`,
`docs/architecture`, `docs/adr`), reproducing the worked example from
MVP.md §4.1 / README.md's claim-block sample almost verbatim:

- `docs/specs/lock-file-schema.md` — claim `[lock-groundness]`
  (`kind: constraint`), declaring `depends: [docs/models/composition-model#6,
  docs/models/execution-model#2.4]` — the **doc-path#anchor** reference
  form, both instances also given matching prose links written as real
  relative markdown paths (`../models/composition-model.md#6`), which
  `checks::normalize_prose_link` resolves against this file's own
  directory to the same path-based ref the `depends` entry carries. Both
  are `depends`, not `because`: the claim's own text ("names bound to
  content identities and exact version strings") states what those two
  model sections *define*, so the claim's meaning requires them to exist.
- `docs/models/composition-model.md` — hosts the `## 6` heading
  `lock-groundness` depends on, plus its own claim `[atom-identity]`
  (`kind: invariant`, `evaluator: proof`, no `depends`/`because`) to
  exercise a second kind/evaluator pair.
- `docs/models/execution-model.md` — hosts the `## 2.4` heading
  `lock-groundness` depends on. No claim block of its own.
- `docs/architecture/system-overview.md` — claim `[system-boundary]`
  (`kind: requirement`, `evaluator: none`), declaring `because:
  [lock-groundness]` — the **claim id** reference form (unaffected by the
  path-refs change: claim ids are bare kebab-case tokens, never paths),
  with a matching prose link. `because`, not `depends`: deleting
  `lock-groundness` would not make the architectural fact ("the boundary
  sits between the atom store and the build graph") false, only
  under-justified — the reverse of `lock-groundness`'s own citations,
  deliberately, so `golden/` exercises both kinds.
- `docs/adr/0001-use-nickel.md` — a decision record. `kinds = []` for this
  genre, so it carries no claim block, which is the compliant state for
  that genre rather than an exception to it.

All five document paths are distinct, all three claim ids are distinct,
every `depends`/`because` entry matches a corresponding prose link, and
every citation resolves.

## Judgment calls made while writing these fixtures

MVP.md was read as the binding contract per the dispatch; nothing here
required a halt, but four points were ambiguous enough that a call had
to be made. Flagging them here rather than deciding silently — three of
the four are now resolved in the spec itself, marked below rather than
removed, since the resolution is part of the record.

1. **`cites`-entry syntax, not just C4 resolution, was made part of C1.**
   MVP.md §1.3 defines two reference *shapes* (bare kebab-case claim id;
   `<doc-stem>#<anchor>`) but never states which check enforces the shape
   itself, as opposed to C4's job of resolving the *target*. `contracts/claim.ncl`'s
   `Ref` contract enforces the shape at C1 time (a `cites` entry with
   spaces or a second `#`, e.g., is a *malformed* block, not merely a
   dangling reference) — reasoning that shape-checking is corpus-independent
   and belongs with the other purely-syntactic checks the contract already
   performs. An implementer could instead defer all `cites` shape
   validation to C4, which would also be a defensible reading.

2. **RESOLVED, now pinned in spec.** MVP.md §3 originally said C5's `L`
   was "restricted to targets that resolve"; this fixture set's own
   reasoning — that a resolution filter makes a dangling `cites` entry
   with a matching prose link fail both C4 and C5, with C5's message
   lying about a divergence that doesn't exist — was accepted, and §3 now
   reads "**ref-shaped**... a syntactic filter, *not* a resolution
   filter" with that exact rationale boxed in. `c4-dangling-cite/`
   isolates C4 exactly as originally designed.

3. **RESOLVED, superseded by MVP.md §1.3's prose-link normalization
   rule.** The bare-literal convention this fixture set originally used
   (`[text](composition-model#6)`, matching a `cites` entry
   character-for-character) is no longer how `golden/` is written. The
   spec now requires a prose href to be **resolved against the citing
   file's own directory** — an ordinary relative markdown link, written
   the way an author actually would (`[text](../models/composition-model.md#6)`)
   — before comparison. `golden/docs/specs/lock-file-schema.md` was
   rewritten to that form; the claim-id reference form
   (`[text](lock-groundness)` in `system-overview.md`) is untouched,
   since claim ids were never path-shaped and this rule doesn't touch
   them.

4. **RESOLVED, now pinned in spec.** MVP.md §1.3's "Anchor derivation"
   states the exact prefix-match rule this fixture set assumed (`## 6` →
   anchor `6`), with the rationale that section numbering churns less
   than heading wording. `golden/`'s headings needed no changes for this.

5. **Reference kinds (`depends`/`because`) retired `cites` corpus-wide.**
   Every fixture that declared `cites` (`blast-doc-anchor/`,
   `c4-dangling-cite/`, `c5-prose-cites-mismatch/`, `golden/`) was
   retyped rather than left on the old field, since `cites` is now an
   unknown field and would fail C1 in every one of them — leaving even
   one on the old name would have been a silent regression the migration
   itself was supposed to prevent. `golden/`'s two citations were
   deliberately typed to different kinds (`depends` in
   `lock-file-schema.md`, `because` in `system-overview.md`, both argued
   inline) so the fixture set exercises both, not just one by default.
   Two new fixtures (`orphaned-because/`, `bare-reference-no-failure/`)
   isolate the properties no existing fixture could: the `Warn`-severity
   split, and the noise-suppression guarantee that a bare reference is
   never resolved against the corpus at all.

## Execution status

**Executed now**, via `nickel export <extracted-block>.yaml --apply-contract
contracts/claim_apply.ncl` for every claim block's YAML across every fixture
(all `docket.ncl` files were also validated against `contracts/docket.ncl`,
which — unlike claim blocks — is plain Nickel and needs no shim). Exit codes
observed, one block per line, `<fixture>_<file>_<block-index-within-file>`:

| block | exit |
|:---|:--:|
| `c1-unknown-field_example_0` | 1 |
| `c2-duplicate-claim-id_a_0` | 0 |
| `c2-duplicate-claim-id_b_0` | 0 |
| `c3-genre-forbids-kind_0001-decision_0` | 0 |
| `c4-dangling-cite_a_0` | 0 |
| `c5-prose-cites-mismatch_a_0` (`target-claim`) | 0 |
| `c5-prose-cites-mismatch_a_1` (`mismatched-claim`) | 0 |
| `explanation-forbids-kinds_0001-decision_0` | 0 |
| `golden_composition-model_0` | 0 |
| `golden_lock-file-schema_0` | 0 |
| `golden_system-overview_0` | 0 |
| `orphan-claim_a_0` | 0 |
| `orphaned-because_a_0` | 0 |
| `bare-reference-no-failure_a_0` | 0 |

`explanation-forbids-kinds`'s own claim block validates cleanly at C1 —
the fixture's failure is entirely in `docket.ncl`, before any block is
ever extracted; see its own section below.

Every `docket.ncl` across all thirteen fixture directories (reference
kinds' `orphaned-because/` and `bare-reference-no-failure/` included, and
`normative-prose-unclaimed/`/`normative-prose-quoted/` from before them)
validated against `contracts/docket.ncl` with exit code 0 —
`explanation-forbids-kinds/` included, since the Nickel contract does not
enforce the derived rule (see its own section below for why).

This confirms exactly the intended shape: **only** `c1-unknown-field`'s
block fails schema validation; every other fixture's claim blocks —
including the ones that fail C2–C5 or the two precondition checks — are
individually well-formed, so C1 does not leak into any fixture but its own.

**Stale note removed.** This section originally said C2–C5, `orphan-claim`,
and `duplicate-stem` had never been run because no program existed yet;
`docket` now implements all of them, `duplicate-stem` is retired, and
every fixture (plus `golden/`) is verified directly against the real
binary as part of each dispatch that touches this manifest — see the
commit history for the actual exit codes, which is the durable record
rather than a table frozen at fixture-authoring time.
