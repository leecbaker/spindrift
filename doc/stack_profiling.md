# Stack-usage diagnosis

This guide documents Quire's macOS stack high-water and call-path profiler,
the KinSNP stack sweep, and static frame inspection. It is developer
documentation, not a renderer feature contract.

For per-document aggregate Grid and float phase costs rather than recursive
stack depth, use the companion [layout profiling guide](layout_profiling.md).

## Current high-water diagnostic

The default-off `stack-profile` feature reads the layout thread's native stack
bounds on macOS. It retains the active recursive path at every new high-water
mark and logs it at 64 KiB intervals. Build the release binary with the
feature, then run it directly:

```sh
cargo build --release --features stack-profile
env -u RUST_MIN_STACK RUST_LOG=quire::stack_profile=info target/release/quire request_kinsnp_default_stack.html request_kinsnp_default_stack.pdf
```

Unsetting `RUST_MIN_STACK` prevents a shell or test configuration from carrying
a larger stack into the default-stack measurement. The Cargo feature, not an
environment variable, enables profiling. Do not pass `--debug`: it enables
unrelated verbose renderer logs. The targeted `RUST_LOG` filter keeps only the
stack diagnostics.

An emitted record has this form:

```text
used_bytes=1861273 stack_bytes=2109440 percent=88.2 path=layout_block_with_descendant_percentage_height_basis > layout_block_flow_children_phase > layout_formatting_box_flow_children[source_index=7]
```

- `used_bytes` is the distance from the pthread stack's top to a local marker
  in that checkpoint.
- `stack_bytes` is the actual macOS worker stack, which can differ from the
  configured `RUST_MIN_STACK` value.
- `percent` is `used_bytes / stack_bytes`.
- `path` is the complete active sequence of profiled recursive boundaries.
  `source_index` identifies the current child or inline-item source position
  at boundaries that traverse indexed children.

The profiler logs after each additional 64 KiB of observed use. Its values are
lower bounds: execution can consume more stack between profiled boundaries.
The complete path is retained whenever a boundary exceeds the prior high-water
mark, but only interval-crossing observations are logged. Non-macOS builds do
not compile the native stack-boundary instrumentation.

## Reproducing the KinSNP threshold

Sweep `RUST_MIN_STACK` in fixed steps and stop at the first successful render.
The recorded investigation used 200 KiB increments above Rust's 2 MiB default:

```sh
for stack_bytes in 2301952 2506752 2711552; do
  echo "RUST_MIN_STACK=${stack_bytes}"
  RUST_MIN_STACK="${stack_bytes}" RUST_LOG=quire::stack_profile=info target/release/quire request_kinsnp_default_stack.html request_kinsnp_default_stack.pdf && break
done
```

Record both the configured value and the profiler's actual `stack_bytes`. A
failed render leaves an unusable output file, so validate only the first
successful output:

```sh
qpdf --check request_kinsnp_default_stack.pdf
pdfinfo request_kinsnp_default_stack.pdf | rg 'Pages|File size'
```

The original investigation needed a 200 KiB sweep because the default stack
overflowed. The current release build renders the checked-in KinSNP request on
the default stack:

| Configured `RUST_MIN_STACK` | Actual worker stack | Last logged profiled high-water |
| ---: | ---: | ---: |
| unset | 2,109,440 B | 1,901,817 B (90.2%) |
| 2,711,552 B | 2,732,032 B | 1,901,817 B (69.6%) |

The resulting PDF has four pages, is 74,504 bytes, and passes `qpdf --check`.
These values are diagnostic baselines, not a supported stack-size requirement:
code generation, operating system details, and input documents can change them.

After staging the direct-DOM traversal controller outside its recursive frame,
a feature-profile run at the same configured value reported a 2,508,569 B
high-water mark (91.8%). The default stack still overflows. This is a
before/after diagnostic measurement, not a new sweep-derived threshold: rerun
the full sweep before updating the first-successful-stack value above.

After the ordered mixed-flow branch was isolated from the direct-DOM phase,
the same feature-profile command reported 2,132,361 B (78.1%). Staging the
block-layout controller then reduced that to 1,901,817 B (69.6%) at the same
configured stack, allowing the default-stack render above to succeed. These
are diagnostic comparisons, not a cross-platform stack-size guarantee.

## Static frame inspection

The high-water diagnostic shows where the running process was deep; it does
not show which functions reserve the stack. On ARM64/macOS, inspect the
optimized binary's function prologue with `otool`:

```sh
nm -nm target/release/quire | rg 'layout_formatting_box_flow_children'
otool -tvV target/release/quire
```

Find the mangled name from `nm` in the disassembly. Sum the initial
register-save allocation and every `sub sp, sp, ...` allocation, including
repeated decrement loops. For example, twelve 4 KiB decrements reserve 48 KiB
before the remaining fixed decrement.

This measures one compiled frame, not live recursive stack use. Combine it
with the high-water measurement; recursion depth, inlining, and temporary
frames determine the actual live stack.

Nightly Rust can emit additional static metadata:

```sh
cargo +nightly rustc --release --features stack-profile --bin quire -- -Z emit-stack-sizes
```

The flag emits object metadata for a stack-size-aware inspection tool; it does
not generate a runtime trace. Cargo features cannot conditionally inject
arbitrary `rustc -Z` flags, and build scripts cannot bridge that restriction.
Do not make `Cargo.toml` nightly-only to automate this command.

## Interpreting call paths

The feature-gated scope guards keep active static labels in thread-local heap
storage and snapshot the whole path only at a new high-water mark. Labels
include source-progress context where available, such as child or inline-item
indices. Combine the captured path with independently measured static frame
sizes to rank repeated large frames separately from one-off frames.

A repeated route that advances source indices indicates expected deep document
traversal. An unchanged repeated route is evidence of recursion without
document progress. The feature adds small guards and calls that affect frames,
so every proposed improvement must be confirmed with an uninstrumented release
render using the same input and stack sweep.
