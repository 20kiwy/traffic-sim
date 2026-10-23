Intentionally empty: the 12 MB openstreetmap-carto symbol set ships on disk at
carto-style/assets/symbols (git-lfs) — too heavy to embed. The embedded
registry starts empty; apps that need POI icons point SymbolRegistry::scan
at the on-disk folder at runtime.
