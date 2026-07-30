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
- **Completeness against *reality* is not.** You cannot count claims
  nobody wrote about behaviour nobody built. Exhaustiveness against reality
  is undecidable, so that half stays a judgment.
- **But completeness against the *code* is measurable**, and that is the
  half that matters for *"the code conforms."* See below.

### Exhaustiveness is undecidable in general and bounded by the corpus in practice

The undecidable question — *did we document everything?* — has a decidable
neighbour: **what code is unaccounted for?** Public surface that no claim
covers is a finite, enumerable set, and every member carries an action:

| the unclaimed surface is | the action |
|:---|:---|
| load-bearing and undocumented | **write the claim** |
| vestigial | **delete the code** |
| legitimately internal | **mark it so** — and that mark is a vouch (below) |

Either of the first two is progress. Which is the point: **an unclaimed
public item is never merely untidy, it is always a signal with a
disposition.**

Note this is the mirror of the evaluator check above — *an evaluator
discharging no claim* — generalized from evaluators to surface. Both
directions of the same relation, and neither is instrumented by
conventional tooling: coverage measures whether code ran, never whether
code is *accounted for*.

**Granularity is public surface, not every function.** Private helpers are
implementation and requiring claims of them would be noise that erodes the
signal. A *public* item is a contract, so a public item covered by no claim
is either an undocumented contract or an over-exposed internal — and both
of those are worth knowing.

**The third case needs a suppression marker or the signal decays.** Some
items are public for mechanical reasons and are genuinely not contracts.
Marking one as internal is an explicit human assertion that it needs no
claim — which is to say **it is a vouch**, attributable like any other, and
counted as one rather than vanishing from the books. Consistent with the
rest of this design: the residue is never hidden, only named.

So the honest statement of the pair:

> **Exhaustiveness against reality is undecidable. Exhaustiveness against
> the code is not — it is the public surface no claim covers, and that set
> is finite, enumerable, and actionable.**

It is a bound rather than an equivalence: claims can exist about things no
code implements yet, so unclaimed surface under-approximates the whole
documentation gap. What it bounds exactly is the gap *over the implemented
surface*, which is precisely the half *"the code conforms"* is about.

This matters because *"the documentation is complete and the code
conforms"* is the intuition worth chasing, and only its second half can
ever be a number.

## Tying claims to the evaluators that discharge them

Line and branch coverage answer *"was this function exercised?"* They say
nothing about whether a **declared claim** is checked, and the two are
orthogonal: a corpus can have complete line coverage and zero claims
verified. So the register needs the link from claim to the specific
evaluators that discharge it — not merely "this claim names an evaluator
kind."

Corpora already do this by hand, in prose, unverifiably — annotations that
name specific test functions beside a constraint, which then drift when
tests are renamed. That is the practice to mechanize.

**The link is declared at the evaluator, and coverage is generated.**
Per principle 2: coverage is a fact about the corpus, so it must be
discovered rather than asserted. A claim block keeps declaring which *kind*
of evaluator it expects; which evaluators actually discharge it is a
finding.

It is declared in the evaluator rather than the document for a second
reason: **tests churn far more than claims**, so the reference belongs in
the artifact that moves. A renamed test cannot silently un-discharge a
claim, because its marker travels with it.

### The marker spans languages, because evaluators are heterogeneous

```rust
// docket: lock-groundness
#[test]
fn ground_values_only() { … }
```

A comment marker, not a language attribute. The reason is not
convenience: a corpus's evaluators are **not all one language**. Proofs
live in Lean, model checks in TLA+ and Alloy, property and unit tests in
the implementation language, examples in CLI transcripts. A Rust attribute
cannot mark a Lean theorem or a TLA+ module; a line comment can mark all of
them.

*This does not contradict "no regular expressions over prose."* A marker is
**structure** — an exact token in a known position. The prohibition is on
inferring structure from prose, which is a different thing.

### The check nobody instruments: evaluators that discharge nothing

The reverse direction is as informative as the forward one. **An evaluator
declaring no claim is suspect** — either it exercises something the corpus
never declared (a missing claim), or it tests an implementation detail
(legitimate, but it should not count toward the metric).

Reported, never failed on. But the ratio matters: a project where most
evaluators discharge no claim has a verification surface disconnected from
its documentation surface, and no aggregate number would reveal it.

### The metric is per kind, and that is the whole point

A single fraction hides precisely the gap worth finding. A corpus can have
every *constraint* discharged and not one *requirement* demonstrated, and
an aggregate would look healthy.

So conformance is reported per kind:

```
requirement   2/3    ( 67%)
invariant     5/7    ( 71%)
constraint   14/14   (100%)
```

**Which is what distinguishes this from code coverage.** Coverage measures
the low-level surface; this measures whether the *high-level declarations*
are checked — and per-kind is the only shape in which that question has an
answer.

If the correspondence between kinds and the three axes of the verification
ceiling holds (see this repository's ledger), then per-kind conformance
reads as **which axis is under-closed**, which makes that correspondence
actionable rather than decorative.

### The same marker reaches load-bearing API surface — at a higher grade

An API can discharge a claim, and it does so **differently in kind** from a
test:

| | what it establishes |
|:---|:---|
| a test | *this case passed* — evidence of conformance |
| **a type** | *the violation does not exist* — enforcement by construction |

A phantom-typed digest disjoint from a backend object id does not *test*
that the two are never confused; it makes the confusion **unrepresentable**.
A set type exposing only a join does not test commutativity; it removes the
means to violate it. A verdict with no boolean conversion does not test that
its residue is non-erasable; erasure fails to compile.

So the register records a **grade**, not a boolean, along the standard
hierarchy:

```
proof  >  type  >  property test  >  example test  >  linter  >  review
```

Mechanically this is the same marker in the same place, which is the point
— one mechanism, two capabilities:

```rust
// docket: czd-oid-disjoint
pub struct Czd<T>(…);
```

**Two things this buys that a boolean cannot.** It distinguishes *how
strongly* each claim is held. And it surfaces a refactoring signal no tool
gives today: **a claim discharged at a lower grade than it could be** — a
constraint guarded by a test where a type could make the violation
unrepresentable. That is a queryable list rather than an insight someone
has to happen to have.

### The hierarchy is normative, not merely descriptive

It is a **preference order**, not a scale for reporting. Where a claim can
be discharged elegantly at a higher grade, it should be — a violation that
cannot be written beats one caught when written.

So a grade gap is not neutral information. The mechanical part is the
signal (*discharged at N; N+1 appears reachable*); the verdict stays human,
because a type-level solution reached by contortion is worse than the test
it replaced. Surfaced by the machine, judged by a person — the same split as
everywhere else here.

**The gradient also predicts which obligations are irreducible**, which is
its most useful property. Types enforce *structural* claims — this cannot be
represented, this cannot be called without that. Proofs establish *semantic*
ones — this holds over all inputs, or all reachable states. So a claim
quantified over all inputs will never reach type-grade, and no amount of
API design will move it. **Counting those tells a project exactly how much
proof it actually needs**, rather than deciding by architectural instinct up
front.

### Where type-discharge's honesty limit sits, stated rather than hidden

The machine confirms the marked item exists and compiles. **It cannot
confirm the type genuinely enforces what the claim says** — a type could be
marked and enforce nothing.

So type-discharge splits: the machine half is *"it exists and the build
passes"*; the faithfulness half is **review-established**. That is exactly
the epistemics of a machine-checked proof, where the checker verifies the
proof and a human verifies the *definition* is faithful. Same honesty
discipline, applied one grade down; it must be reported as
review-established rather than silently counted as machine-established.

A partial machine check is possible per language — confirming the marked
item is a type rather than a function — but it is a per-language
enhancement, not a precondition.

### Why a deterministic index rather than careful reading

*"Which API surfaces enforce which invariants"* is an exhaustive
cross-cutting question over a large corpus, and it is precisely the shape
where a language model returns a confident partial answer. There is no
gradient toward exhaustiveness in generation, so the omissions are
invisible from inside.

The consequence is about review, not tidiness. A human auditing machine-authored
work needs to establish *"was this claim actually enforced?"* — and the cost
of that answer determines whether review is feasible at all. **A register
makes it a query; without one it is a code read.** Given that code review is
slow by definition, moving the audit from diff level to declaration level is
what makes the human side of the loop tractable.

### Out of MVP scope, but it disturbs nothing

This needs the evaluator runner, so it lands after the index. It requires
**no change to the claim block** — the block declares an expected evaluator
kind; discharge is discovered separately. The MVP's format is
forward-compatible as written.

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
