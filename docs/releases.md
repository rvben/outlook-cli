# Releases

The release workflow follows rumdl's seven-target wheel matrix:

| Platform | Architecture | Wheel baseline |
| --- | --- | --- |
| Windows | x64 | MSVC |
| macOS | Intel x64 / Apple Silicon ARM64 | Maturin's deployment target |
| Linux / WSL | x64 / ARM64 | manylinux2014 (glibc 2.17+) |
| Alpine Linux | x64 / ARM64 | musllinux 1.2 |

Linux wheels build in the corresponding PyPA containers on native architecture
runners. Every wheel is installed using `uv tool install --no-build --no-index`
and its CLI version and schema are checked. Musl wheels run in Alpine. The
standalone executable is also tested outside the wheel installation tree.

All seven wheels, the source package, all executable archives, and their checksums
must pass validation before the `release-packages` artifact is produced. A partial
matrix cannot enter the publish job. Existing CI tests, including the Windows
PowerShell mock bridge tests, run before packaging.

## Prepare and build

Vership owns versioning, changelog generation, release commits, tags, and pushes.
From a clean main branch, prepare the next patch with:

```sh
vership bump patch --prepare
```

For a version already set in the manifests, use `vership release --prepare`
instead. Review the staged paths before committing any local working material;
release commits must exclude design reports and screenshots.

After reviewing the release commit, `vership release` runs its checks and pushes
the branch and version tag. The tag starts the seven-platform build. Tags do not
automatically publish to registries.

```sh
vership release
gh run watch RUN_ID --exit-status
gh run download RUN_ID --name release-packages --dir dist/VERSION
python scripts/release.py verify dist/VERSION --require-tag vVERSION
```

A build-only preview is also available with
`gh workflow run release.yml --ref main -f publish=false`.

## Publish

For automated publication, configure repository secrets `CARGO_REGISTRY_TOKEN`
and `PYPI_API_TOKEN`, then explicitly dispatch `Release` on the version tag with
`publish=true`. The workflow rebuilds and verifies the matrix, checks the tag and
both credentials, publishes crates.io and PyPI packages, and creates a GitHub
release with standalone archives. Credentials are not needed for build-only runs.

When publishing locally with existing credentials, use the verified artifact from
the **same release commit**, without rebuilding just the host wheel:

```sh
cargo publish --locked
uv tool run --from twine twine check dist/VERSION/*.whl dist/VERSION/outlook_cli_rs-*.tar.gz
uv tool run --from twine twine upload --non-interactive dist/VERSION/*.whl dist/VERSION/outlook_cli_rs-*.tar.gz
```

Run `vership verify VERSION` after publication to check the tag, GitHub release,
crates.io, and PyPI. Also verify published asset SHA-256 digests. Never replace a
published version or move its tag. If anything was published, prepare a new patch
version for fixes; do not rerun the publish job against an already published
version. If nothing was published, repair the build and retry the same version.
