# MVP specification

The buildable contract for docket's first version. Scope is fixed by
[README.md](README.md) §MVP scope; this document states it precisely
enough to implement against without further design decisions.

**If something here is ambiguous, halt and ask rather than deciding.** A
gap filled by invention is worse than a gap reported.

---

## 1. The claim block

### 1.1 Syntax

A claim block is a fenced code block whose info string is exactly
`claim`, appearing in a markdown file. Its content is YAML.

````markdown
```claim
kind: constraint
evaluator: property-test
cites: [composition-model#6, execution-model#2.4]
```
````

**The claim's `id` is not in the block.** It is taken from the nearest
preceding heading whose text is exactly a bracketed kebab-case token:

```markdown
### [lock-groundness]
```

Rationale: the id already exists in prose as the human-readable anchor,
and duplicating it into the block would create precisely the divergence
surface this tool exists to remove. One statement, one place.

A claim block with no such preceding heading in the same file is an
error (`orphan-claim`).

### 1.2 Fields

| field | required | type |
|:---|:--|:---|
| `kind` | yes | one of `requirement`, `invariant`, `constraint` |
| `evaluator` | yes | one of `proof`, `model-check`, `type`, `property-test`, `test`, `example`, `none` |
| `cites` | no (default `[]`) | array of refs |

Unknown fields are an error, not ignored. Tolerating them would let a
typo'd field name silently carry no meaning.

### 1.3 Reference syntax

A `cites` entry is either:

- **a claim id** — kebab-case, resolving to a claim elsewhere in the
  corpus: `lock-groundness`
- **a document anchor** — `<doc-stem>#<anchor>`, resolving to a heading in
  a corpus document: `composition-model#6`

Document stems are file basenames without extension, and must be unique
corpus-wide (checked; see §3). Stems are **not** constrained to
kebab-case — `README`, `MVP` are valid stems.

**Ref syntax is enforced by the contract, not by C4.** A `cites` entry
whose *shape* is invalid — whitespace, a second `#`, an empty half — is a
malformed block and fails **C1**. C4 therefore only ever operates on
well-formed refs, and "does this resolve" presupposes "is this a ref."

**Prose-link normalization.** A markdown href in prose is normalized to a
ref before comparison (C5): strip any directory prefix and the `.md`
extension to leave the document stem, retain any `#fragment`, and treat a
fragment-only href as referring to the containing document. So
`[…](../models/composition-model.md#6)` normalizes to
`composition-model#6`.

This is load-bearing rather than cosmetic: **real corpora link by relative
path with an extension**, and without normalization those links would never
match a `cites` entry, making C5 fire on every correctly-linked claim. The
bare-literal form used in `fixtures/` is the degenerate case of the same
rule.

**Anchor derivation.** An anchor `A` matches a heading iff the heading's
text — after stripping `#` markers and leading whitespace — begins with
`A` followed by either end-of-string or a non-alphanumeric character. So
`composition-model#6` matches `## 6. The fact-set: …` and
`execution-model#2.4` matches `### 2.4 Something`, while `#6` does **not**
match `## 60. …`.

Section numbers rather than slugified heading text, deliberately: heading
*wording* churns far more often than section *numbering* in the corpora
this tool targets, so numbers are the more stable anchor. The failure mode
when a document is renumbered is loud — C4 fails immediately — rather than
silent.

## 2. Configuration

A repository declares its own genre hierarchy in `docket.ncl` at the
repository root. Genres are **not** built in.

**A path matching more than one genre is a configuration error (exit 2),
not a precedence question.** No first-match, no most-specific-wins. Both
conventions are defensible and both surprise somebody — and note that
first-match is the *opposite* of `.gitignore`, where later patterns
override — so rather than pick an order and document it, the config is
required to be unambiguous. Making patterns disjoint is trivial; silently
assigning a document to the wrong genre is exactly the failure this tool
exists to prevent.

```nickel
{
  genres = [
    { path = "docs/specs/**",        kinds = ["constraint"] },
    { path = "docs/models/**",       kinds = ["invariant"] },
    { path = "docs/architecture/**", kinds = ["requirement"] },
    { path = "docs/adr/**",          kinds = [] },   # no claims permitted
  ],
  # Files matching no genre are not scanned.
}
```

`kinds = []` means the genre may hold **no** claim blocks — the mechanism
by which a decision record is prevented from carrying normative content.

## 3. Checks

All five run on every invocation. Each failure names the file, the line,
and the offending value.

| id | check | failure means |
|:---|:---|:---|
| `C1` | every claim block validates against the contract | malformed or unknown field |
| `C2` | claim ids are unique corpus-wide | two homes for one id |
| `C3` | **`kind` ∈ the genre's permitted kinds for that path** | **a genre violation — a fact in the wrong home** |
| `C4` | every `cites` target resolves | dangling reference |
| `C5` | prose links and `cites` agree, per claim | the graph and the prose have diverged |

**C5, precisely.** For a claim, let `L` be the set of markdown link
targets appearing in its prose body (from its id heading to the next
heading of the same or higher level, excluding the claim block itself),
restricted to targets that are **ref-shaped** per §1.3 — a syntactic
filter, *not* a resolution filter. Then `L` and `cites` must be equal as
sets. A link to an external URL is ignored, since it is not ref-shaped.
Rationale: `cites` is authoritative for the graph, but a reader follows
prose, so the two must not disagree.

> **Syntactic, not resolution — and the reason is not isolability.** An
> earlier draft said *"targets that resolve."* Under that reading a
> dangling `cites` entry with a matching prose link fails **both** C4 and
> C5, because the unresolvable link would be excluded from `L`. Two errors
> for one defect is the lesser problem. The real problem is that **C5's
> message would lie**: prose and `cites` agree perfectly in that case —
> both point at nothing — and reporting a prose/`cites` divergence
> misdiagnoses it. C4 owns "this target does not exist"; C5 owns "the two
> representations disagree." Keeping them syntactically separate keeps both
> diagnoses true.

Also checked, as a precondition rather than a numbered check: document
stems are unique corpus-wide (`duplicate-stem`), since references depend
on it.

## 4. Outputs

### 4.1 The index

Emitted to stdout as JSON, or to a path with `--out`. **Never committed** —
it is a pure projection of the source.

```json
{
  "claims": {
    "lock-groundness": {
      "file": "docs/specs/lock-file-schema.md",
      "line": 88,
      "kind": "constraint",
      "evaluator": "property-test",
      "cites": ["composition-model#6", "execution-model#2.4"]
    }
  },
  "documents": {
    "lock-file-schema": { "file": "docs/specs/lock-file-schema.md", "genre": "docs/specs/**" }
  }
}
```

### 4.2 Blast radius

```
docket blast <claim-id>
```

Prints, transitively, every claim and document that cites the given claim
— the set a reviewer must re-check if it changes. Cycles are reported, not
followed twice.

## 5. Exit codes

| code | meaning |
|:--|:---|
| 0 | all checks pass |
| 1 | one or more checks failed |
| 2 | usage or configuration error (bad `docket.ncl`, unreadable path) |

Suitable as a CI gate with no wrapper.

## 6. Deliberately out of scope

The evaluator runner, the verdict register, the stability metric,
generated reference output, and signing. Each needs the index to exist
first, and none of them changes the shape above.

## 7. Implementation notes

- **Language:** the repository's choice; no constraint from this spec. The
  checks are pure functions over parsed input, so the natural shape is a
  library plus a thin CLI.
- **Nickel is required** for the contract and `docket.ncl`. Validation
  should invoke Nickel rather than reimplementing its checking.
- **The extractor must not use regular expressions over prose.** Parse
  markdown to a document tree, then walk it. Fenced blocks and heading
  levels are structure, and regex over structure is the brittleness this
  tool is meant to displace.
- **Test corpus:** a fixture directory of small markdown files, one per
  check, each a minimal instance that must fail exactly that check — plus
  a golden corpus where everything passes. Checks are the product, so the
  fixtures are the specification of correctness.
