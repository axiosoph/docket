# Spec

<a id="html-claim"></a>

The system MUST persist keyed data across restarts.

```claim
kind: requirement
evaluator: test
```

### [depends-on-html]

Depends on [the html-anchored claim](#html-claim) above — the anchor
form, resolving the same way it would for any other definition form.

```claim
kind: constraint
evaluator: test
depends: [html-claim]
```
