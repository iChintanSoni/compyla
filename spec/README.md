# Compyla Spec Artifacts

The source of truth for Compyla's behavior (architecture.md D16). Implementations conform to these;
these do not describe any one implementation.

| Artifact | Defines |
|----------|---------|
| [../docs/markdown-spec.md](../docs/markdown-spec.md) | The dialect, the IR mapping, the four format profiles, operations, diagnostics, limits, versioning |
| [ir.schema.json](ir.schema.json) | The IR (`irVersion` 0.1) |
| [diagnostic.schema.json](diagnostic.schema.json) | The diagnostic object every operation returns |
| [diagnostics.json](diagnostics.json) | Registry of every diagnostic code and operation error |
| [template-report.schema.json](template-report.schema.json) | Output of `inspect_template` |
| [conformance/](conformance/README.md) | Fixtures: the executable definition of correct behavior |
| [tools/](tools/) | Spec tooling: the prototype oracle and the artifact checker |

All schemas are JSON Schema 2020-12.

## Changing the spec

1. Change `docs/markdown-spec.md` and, where affected, the schemas and the registry.
2. Add or update fixtures that encode the change. A behavior change without a fixture is not done.
3. Record the change in markdown-spec.md §0.2 (or the next version's change table) and bump the
   version per §14.
4. Run `node spec/tools/check.mjs` (artifact consistency) and `node spec/tools/oracle.mjs`.
