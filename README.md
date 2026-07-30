# docket

**A register of claims across a documentation corpus.** Requirements,
invariants, and constraints get an identifier, exactly one home, and a
named evaluator — so that *"is this project stable?"* becomes a computed
number with an enumerated residue, instead of a feeling.

> Status: **pre-MVP.** This README is the design; the tool does not exist
> yet. Name is provisional.

---

## The problem

A specification-first project accumulates several genres of document —
decision records, formal models, normative specifications, architecture
overviews, user documentation. Each answers a different question. In
principle they compose; in practice they **drift**, and the drift is
structural rather than careless:

- **Duplication is the drift surface.** When the same fact is stated in
  four documents, there are four places to update and four places to
  diverge. Divergence is then discovered by a reader, months later, and
  usually by being misled.
- **Status is asserted, so it rots.** A constraint annotated
  `VERIFIED: unverified` looks identical to a discharged one at a glance,
  and nothing forces the annotation to match reality.
- **Conventions decay under machine authorship.** Documentation genres are
  usually kept apart by habit. Habits are not something a language model
  follows reliably, and generation is *additive by construction* — an LLM
  corrects by appending, rarely by cutting. So an agent-maintained corpus
  accretes correction layers and grows monotonically, appearing diligent
  while getting worse.

The last point is why this is a tool rather than a style guide. Under
heavy machine modification, anything that depends on discipline will fail.

## Two principles

Everything here follows from these:

> **1. Anything an agent could get wrong must be checked by a gate, not
> documented as a convention.**
>
> **2. Anything an agent could assert must be generated, not written.**

Applied: genre boundaries become a gate rather than an editorial norm;
claim status becomes a build artifact rather than an annotation; reference
documentation becomes a projection of source rather than prose to
maintain.

## Genres, and one home per fact

Each genre answers exactly one question, and may hold exactly one kind of
claim. **Genres may cite; they may never restate.**

| genre | answers | claim kind |
|:---|:---|:---|
| decision record | *why* | none — terminal justification, immutable |
| formal model | *what law holds* | `invariant` |
| specification | *what must be true* | `constraint` |
| architecture overview | *how it fits together* | `requirement` |
| user documentation | *how it is used* | (evaluator: executable example) |
| reference | — | **generated**, never written |

Two consequences worth stating plainly:

**A decision record that carries normative content will need revising**,
which its genre forbids — a decision is superseded, not amended. Revision
layers inside a decision record are the symptom of a genre violation, and
they are how the most-drifted document in a corpus usually got that way.

**An architecture overview's drift risk is proportional to the substance
it holds.** One that carries diagrams, boundaries, and pointers cannot
contradict anything. One that restates invariants will, and its readers
will trust the stale copy.

## The claim block

Prose stays prose. Machine-tractable metadata rides alongside it in a
fenced block, validated by a committed Nickel contract:

````markdown
### [lock-groundness]

Every lock value MUST be ground: names bound to content identities and
exact version strings.

```claim
kind: constraint
evaluator: property-test
cites: [composition-model#6, execution-model#2.4]
```
````

Fenced blocks rather than inline XML or HTML attributes, for one reason:
**a language model writes a fenced block with a schema correctly far more
often than it writes nested markup.** Unclosed tags and attribute drift
are precisely the error class this tool exists to eliminate.

The *query* surface is a separate concern. Any pure projection of the
source will do — index, HTML, graph — because a pure transformation is
equally safe to operate on either side.

## What the block holds, and what it must not

| in the document | in the generated register |
|:---|:---|
| `id`, `kind`, `evaluator` **name**, `cites` | the evaluator's **verdict** |
| stable across runs | recomputed every run |

**Verdicts are never written.** The register is produced by running the
evaluators, so *the absence of a passing evaluator is the unverified
state* — discovered rather than asserted. Nothing can drift from reality,
because nothing claims anything about reality.

The register is a pure projection and is therefore **not committed**.

## Links are data

`cites` is the graph edge; a markdown link in prose is presentation. A
lint checks they agree, so they cannot diverge — and a normative change's
**blast radius** is computed over `cites`, never by pattern-matching
prose. Change a claim, and the set of documents that must be re-checked is
a query result.

## "Stable", defined honestly

> **Stable = every documented claim has a passing evaluator, and the
> residue is enumerated rather than unknown.**

Half of that is measurable and half is not, and conflating them
overstates the guarantee:

- **Conformance is measurable.** It is a fraction over the register, with
  a named residue. It moves monotonically as work lands.
- **Completeness is not.** You can count claims that have homes; you
  cannot count claims nobody wrote. Exhaustiveness against reality is
  undecidable, so completeness stays a judgment — bounded by a readiness
  criterion, not proved.

This matters because *"the documentation is complete and the code
conforms"* is the intuition worth chasing, and only its second half can
ever be a number.

## Generality

Nothing here is specific to one project, and genre boundaries differ
between corpora. So **genres are configuration**: a declared mapping from
path pattern to permitted claim kinds. A repository states its own
hierarchy, and the gate enforces whatever it stated.

The payoff of sharing the tool across projects is not code reuse. It is
**one definition of stable, comparable across corpora** — the same metric
measured in more than one instance.

## MVP scope

Deliberately smaller than the design above. The register and the metric
need something to measure; the writing pass needs only tractability.

**In:**

1. A Nickel contract for the claim block.
2. An extractor: pull fenced `claim` blocks from markdown.
3. An index: `id → (file, kind, evaluator, cites)`.
4. Five checks:
   - blocks validate against the contract
   - ids are unique corpus-wide
   - **`kind` is permitted by the genre its path declares**
   - every `cites` target resolves
   - prose links agree with `cites`
5. A blast-radius query: given a claim id, what cites it, transitively.

**Out, until there is something to measure:** the evaluator runner, the
verdict register, the metric, generated reference output, signing.

Check 3 carries most of the value. It makes the genre hierarchy
**mechanically enforced** rather than editorially intended — and with it,
duplication is *structurally prevented* rather than periodically measured,
because a genre that may only cite has nowhere to put a second copy.

## Non-goals

- Judging whether a document is *correct*. That stays human, and stays
  hard.
- Detecting semantically duplicated claims — the same fact under two ids.
  The genre gate removes most of the room for it; the remainder is a
  judgment call and should not be automated into a false negative.
- Replacing prose. Every claim is a sentence a person reads; the block
  beside it is for the machine.
