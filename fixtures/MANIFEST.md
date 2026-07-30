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
| `c2-duplicate-claim-id/` | `C2` | `docs/specs/a.md` and `docs/specs/b.md` both head a claim `[dup-id]`. Each claim block is independently well-formed, in a genre that permits its kind, with no `cites` — so nothing but the id collision fails. |
| `c3-genre-forbids-kind/` | `C3` | `docket.ncl` declares `docs/adr/**` with `kinds = []` (MVP.md §2's "no claims permitted" form). `docs/adr/0001-decision.md` declares a `requirement` claim there anyway. |
| `c4-dangling-cite/` | `C4` | `docs/specs/a.md`'s claim cites `nonexistent-claim`, which resolves to nothing in the corpus. A prose link `[related work](nonexistent-claim)` with the identical target is present so `C5` (link/cites agreement) still passes — see the C5-vs-C4 judgment call below. |
| `c5-prose-cites-mismatch/` | `C5` | `docs/specs/a.md` declares `[target-claim]` (cited, and it resolves — so `C4` passes) and `[mismatched-claim]`, which cites `[target-claim]` but whose prose body carries no matching link. `L = {}`, `cites = {target-claim}`, sets disagree. `[target-claim]` exists specifically so the citation resolves and `C4` cannot also fire. |
| `orphan-claim/` | `orphan-claim` | `docs/specs/a.md` is a claim block with no preceding heading in the file at all — the minimal case of "no `[id]`-shaped heading precedes it" (MVP.md §1.1). |
| `golden/` | — (all pass) | See below. |

**`duplicate-stem/` is retired**, not just its row here. MVP.md §1.3 was
amended once a real corpus produced three `README.md` files under one
genre (`docs/models/{lean,lean-surety,tla}/README.md`), indistinguishable
by basename: document identifiers are now corpus-relative paths, unique
by construction, so there is nothing left for a stem-collision check to
detect. The fixture directory and its two empty files are deleted.

## `golden/`

One document per MVP.md §2's example genre (`docs/specs`, `docs/models`,
`docs/architecture`, `docs/adr`), reproducing the worked example from
MVP.md §4.1 / README.md's claim-block sample almost verbatim:

- `docs/specs/lock-file-schema.md` — claim `[lock-groundness]`
  (`kind: constraint`), citing `docs/models/composition-model#6` and
  `docs/models/execution-model#2.4` — the **doc-path#anchor** reference
  form, both instances also given matching prose links written as real
  relative markdown paths (`../models/composition-model.md#6`), which
  `checks::normalize_prose_link` resolves against this file's own
  directory to the same path-based ref the `cites` entry carries.
- `docs/models/composition-model.md` — hosts the `## 6` heading
  `lock-groundness` cites, plus its own claim `[atom-identity]`
  (`kind: invariant`, `evaluator: proof`, no `cites`) to exercise a second
  kind/evaluator pair.
- `docs/models/execution-model.md` — hosts the `## 2.4` heading
  `lock-groundness` cites. No claim block of its own.
- `docs/architecture/system-overview.md` — claim `[system-boundary]`
  (`kind: requirement`, `evaluator: none`), citing `lock-groundness` — the
  **claim id** reference form (unaffected by the path-refs change: claim
  ids are bare kebab-case tokens, never paths), with a matching prose
  link.
- `docs/adr/0001-use-nickel.md` — a decision record. `kinds = []` for this
  genre, so it carries no claim block, which is the compliant state for
  that genre rather than an exception to it.

All five document paths are distinct, all three claim ids are distinct,
every `cites` entry matches a corresponding prose link, and every
citation resolves.

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
| `golden_composition-model_0` | 0 |
| `golden_lock-file-schema_0` | 0 |
| `golden_system-overview_0` | 0 |
| `orphan-claim_a_0` | 0 |

Every `docket.ncl` (all eight fixtures) validated against
`contracts/docket.ncl` with exit code 0.

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
