# xml5ever duplicate-attribute reproduction

This standalone project reproduces xml5ever incorrectly rejecting this valid
namespace-qualified XML:

```xml
<x xmlns:n1="http://www.w3.org" xmlns="http://www.w3.org">
  <good n1:a="2" a="1" />
</x>
```

The two attributes have different expanded names: `a` has no namespace and
`n1:a` is in `http://www.w3.org`. XML permits both attributes on the same
element.

Run it with:

```sh
cargo run
```

Expected behavior: the program exits successfully.

Actual behavior with xml5ever 0.39.0 and current upstream `main`: the program
panics because xml5ever reports `Duplicate attribute`.
