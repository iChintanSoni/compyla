# Security policy

Compyla reads untrusted input: markdown, IR JSON, images and template packages (zip files of XML).
It is designed to refuse hostile input safely (markdown-spec.md §12): size and inflation limits,
no DTDs, no unsafe paths, no macros. A way around those protections is a vulnerability.

## Reporting

**Report privately, never in a public issue:** use
[GitHub's private vulnerability reporting](https://github.com/iChintanSoni/compyla/security/advisories/new).

Include:
- what an attacker controls (the markdown, an image, the template…), and what they gain: a crash,
  a hang, memory exhaustion, a file read or write outside the expected place, or code execution;
- the smallest input that reproduces it, and the version or commit;
- how you ran it (CLI, a library call, a host).

## What to expect

This is a small project. The maintainer aims to acknowledge a report within a week, to agree on a
fix and a disclosure date with you, and to credit you in the advisory unless you'd rather not be.

## Supported versions

Before 1.0, only the latest commit on `main` and the latest release receive fixes.

## Scope

In scope: anything that processes input — parsing, validation, the renderers, template reading and
inspection, and the hosts. Out of scope: output a viewer application mishandles without a flaw in
what Compyla wrote (report those as a [real-viewer finding](https://github.com/iChintanSoni/compyla/issues/new/choose)).
