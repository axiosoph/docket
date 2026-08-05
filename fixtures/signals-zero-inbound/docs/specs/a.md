### [base]

A base fact other claims in this corpus build on.

```claim
kind: constraint
evaluator: test
```

### [derived]

Builds on [base](#base): `derived` depends on it, but nothing in this
corpus cites `derived` itself — its in-degree is zero even though its
own out-degree is one, proving the two are independent axes rather
than one number read two ways.

```claim
kind: constraint
evaluator: test
depends: [base]
```

### [standalone-invariant]

A self-contained forbidden state: nothing else in this corpus needs to
depend on it for it to hold. Zero inbound and zero outbound — the
`docket signals` candidate this fixture exists to isolate.

```claim
kind: constraint
evaluator: test
```
