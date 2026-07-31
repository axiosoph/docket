# Bold-form false-positive floor

**[real-definition]**: The only genuine definition in this corpus.

```claim
kind: constraint
evaluator: test
```

**Note**: ordinary bold text with no bracket-kebab id inside it is not
a definition, direct colon or not.

As required by **[real-definition]** above, a mid-sentence citation of a
real id — bold, bracketed, but not at line start — MUST NOT be treated
as a second definition of the same id.

**[not-a-definition]** starts a line but is not immediately followed by
punctuation, so it is not recognized either.

- **[no-unpublished-dependency]** MUST be enforced: a list marker
  precedes the bold span on the same line, so it is not line start.
