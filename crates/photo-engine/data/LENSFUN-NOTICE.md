# Lens data

`lensfun.tsv.z` is distilled from the lens database of the
[lensfun project](https://lensfun.github.io/), at commit
`bbd4332a9ec566fd9aa548c9e0d8ced238c56261` (24 September 2026). The database is
licensed under [Creative Commons Attribution-ShareAlike 3.0](https://creativecommons.org/licenses/by-sa/3.0/),
and so is this table.

The names of cameras and lenses and their calibration coefficients are lensfun's,
contributed by its many calibrators. The table keeps the rectilinear lenses and, for
vignetting, only the farthest calibrated focus distance. Otherwise it is only
reformatted, by `tools/lensfun_extract.py`. Duckfoot reads the data and does not use the
lensfun library (DEC-8).

To refresh it, clone lensfun and run:

    python3 tools/lensfun_extract.py path/to/lensfun/data/db
