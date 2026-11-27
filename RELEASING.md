# Releasing

1. Update `version` in `Cargo.toml` (workspace), `crates/ferroreport-cli/Cargo.toml` (the
   `ferroreport` dependency), `bindings/wasm/package.json`, and for Java `bindings/java/pom.xml`
   and `Ferroreport.VERSION` (the engine release the Java library downloads). Then the version
   people read, which `grep -rn '1\.2\.0' README*.md bindings/*/README.md site/` lists (put
   the old version in): the status line of both READMEs, the Maven coordinates in the READMEs,
   `site/docs.html`, `site/migrate.html` and the Java snippet of `site/index.html` (each also
   under `site/es/`), "the current one" in `site/compare.html` and `site/es/compare.html`,
   `site/llms.txt`, and the `version` of the JSON-LD in both `index.html`.
2. Check what the release changes in the output: `cargo build --release && python3 tools/compat.py
   --old <previous version>` (see CONTRIBUTING.md). Every template `target/compat/report.md`
   lists as changed should be explained under `## Unreleased` in `CHANGELOG.md`, which becomes
   the release's section; within 1.x a change may only bring the output closer to
   JasperReports.
3. Commit, then tag and push: `git tag v1.0.0 && git push origin v1.0.0` (or publish a release
   with a new tag on GitHub).

The [release workflow](.github/workflows/release.yml) then publishes:

| Where | What | Setup |
|---|---|---|
| GitHub Releases | `ferroreport` CLI for Linux (x86_64, aarch64, static), macOS (x86_64, arm64), Windows | none |
| GitHub Container Registry | `ghcr.io/cristianencarnacion/ferroreport` (HTTP API, amd64 + arm64) | none (make the package public once in its settings) |
| npm | `ferroreport` (WebAssembly: Node.js, Deno, Bun, browsers) | variable `PUBLISH_NPM=true` |
| PyPI | `ferroreport` (wheels for Linux, macOS, Windows; Python 3.9+) | variable `PUBLISH_PYPI=true` |
| crates.io | `ferroreport` (library) and `ferroreport-cli` | variable `PUBLISH_CRATES=true` |
| Maven Central | the Java library, as `io.github.cristianencarnacion:ferroreport` | variable `PUBLISH_MAVEN=true` and four secrets (below) |
| Visual Studio Marketplace | the VS Code extension, as `cristianencarnacion.ferroreport` | variable `PUBLISH_VSCODE=true` |
| JitPack | the Java library, as `com.github.cristianEncarnacion:ferroreport:<tag>` | none: JitPack builds `bindings/java` from the tag the first time someone asks for it (`jitpack.yml`) |

Repository variables live in *Settings → Secrets and variables → Actions → Variables*.

## One-time registry setup

All registries use **trusted publishing**: GitHub Actions proves its identity to the registry,
so no long-lived token is stored anywhere. Each registry needs the repository
`cristianEncarnacion/ferroreport`, the workflow `release.yml` and the environment shown below.

Current state:

| Registry | Trusted publisher | Notes |
|---|---|---|
| crates.io | configured for `ferroreport` and `ferroreport-cli` (environment `crates-io`) | the `CARGO_REGISTRY_TOKEN` secret is only a fallback and can be deleted |
| npm | configured (environment `npm`, may publish) | npm validates it with the first release that uses it; then the `NPM_TOKEN` secret can be deleted |
| PyPI | configured (environment `pypi`) | 1.0.0 was published through it |
| Maven Central | no trusted publishing there: a user token and a signing key, as secrets of the environment `maven-central` | configured; 1.0.1 was published through it. Signing key `E43D097BF9C5E3407DF285CB2347362558F83FA0`, public half on keyserver.ubuntu.com and keys.openpgp.org |
| Visual Studio Marketplace | configured: publisher `cristianencarnacion`, policy for `release.yml` in the environment `vscode-marketplace` | the job runs `vsce publish --oidc`; the first publish activates the policy |

How each one was set up, for reference:

- **PyPI** (works before the first release): on pypi.org, *Your account → Publishing → Add a new
  pending publisher*: project `ferroreport`, owner `cristianEncarnacion`, repository
  `ferroreport`, workflow `release.yml`, environment `pypi`.
- **npm**: the first version must be published with a token, because npm can only trust a
  package that exists. Create a granular access token with publish rights on npmjs.com, save it
  as the repository secret `NPM_TOKEN`, release, then in the package settings add the trusted
  publisher (workflow `release.yml`, environment `npm`) and delete the secret.
- **crates.io**: same as npm. Create an API token (scope `publish-new`, `publish-update`) on
  crates.io, save it as the secret `CARGO_REGISTRY_TOKEN`, release, then add the trusted
  publisher for both crates (workflow `release.yml`, environment `crates-io`). The workflow
  prefers trusted publishing and only falls back to the secret.
- **Maven Central** (central.sonatype.com has no trusted publishing, so this one uses secrets):
  1. Sign in to [central.sonatype.com](https://central.sonatype.com) with the GitHub account
     `cristianEncarnacion`: the namespace `io.github.cristianencarnacion` is then verified
     already (it is the one in `bindings/java/pom.xml`).
  2. *View Account → Generate User Token*. Save its two halves as the secrets
     `MAVEN_CENTRAL_USERNAME` and `MAVEN_CENTRAL_PASSWORD`.
  3. Every file on Maven Central is signed. Make a key for it on your own computer
     (`gpg --quick-generate-key "Cristian Encarnacion <cencarnacioninf13@gmail.com>" ed25519 sign 0`),
     publish its public half (`gpg --keyserver keys.openpgp.org --send-keys <KEY ID>`, then
     confirm the e-mail keys.openpgp.org sends), and save the private half
     (`gpg --armor --export-secret-keys <KEY ID>`) as the secret `MAVEN_GPG_PRIVATE_KEY` and its
     passphrase as `MAVEN_GPG_PASSPHRASE`.
  4. Put the four secrets in the environment `maven-central` (*Settings → Environments*), and
     set the repository variable `PUBLISH_MAVEN` to `true`. The next release then publishes the
     library; a version already there is skipped. `mvn -Prelease -Dgpg.skip package` (run by
     CI) checks that the sources and javadoc jars build.

