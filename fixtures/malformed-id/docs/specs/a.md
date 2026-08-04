# Malformed ids

**[boundary-L1-concerns]**: L1 (atom) owns content addressing and lock
verification — the real-corpus shape this diagnostic exists to catch:
an otherwise-kebab id with one stray uppercase segment.

### [Daemon-Discovery]

A heading-form malformed id: an uppercase segment fails the id grammar
just as loudly in this form as in bold form.

**[Note to reader]**: A bracket with a space in it reads as prose, not
an attempted id — this must not fire `malformed-id`.

### [registered-claim]

A normal, well-formed definition, present only for contrast: this one
stays completely silent.

```claim
kind: constraint
evaluator: test
```
