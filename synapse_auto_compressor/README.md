# Auto Compressor

See the top level readme for information.


## Publishing to PyPI

Bump the version number and run from the root directory of the repo:

```
docker run -it --rm -v $(pwd):/io -e OPENSSL_STATIC=1 konstin2/maturin publish -m synapse_auto_compressor/Cargo.toml --cargo-extra-args "\--features='openssl/vendored'"
```
