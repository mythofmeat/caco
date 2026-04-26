Goal

Separate user-selected sourceport overrides from automatically detected sourceport compatibility metadata.

Currently, imported Doomworld/Doom Wiki metadata can set `custom_sourceport`, for example `dsda-doom`. This causes WADs that merely mention or target DSDA-compatible ports to override the user’s preferred default sourceport, such as `nyan-doom`, even though `nyan-doom` is already treated as part of the `dsda` family.

Change the model so:

- `custom_sourceport` means “the user explicitly chose this executable for this WAD.”
- Automatically detected/imported sourceport compatibility is stored as `required_sourceport_family`.
- Launch resolution uses sourceport family compatibility so preferred forks like `nyan-doom` can satisfy `dsda` requirements.


Context

caco already has sourceport family definitions in:

- `crates/caco-core/src/sourceports.rs`

The `dsda` family already includes:

- `dsda-doom`
- `nyan-doom`
- `nugget-doom`
- `prboom+`
- `prboom-plus`
- `glboom+`
- `glboom-plus`

The current launcher sourceport precedence in `crates/caco-core/src/player.rs` is effectively:

1. CLI `--sourceport`
2. WAD `custom_sourceport`
3. global config `sourceport`

This is mostly correct, but imported metadata is currently being written into `custom_sourceport`, which makes imported recommendations behave like user overrides.

Doomworld import currently persists detected thread sourceports into `custom_sourceport` in:

- `crates/caco-sources/src/import_service.rs`

Doomworld parsing normalizes mentions such as “DSDA” / “DSDA-Doom” to `dsda-doom` in:

- `crates/caco-sources/src/doomworld/parser.rs`

That behavior is useful, but the normalized sourceport should be converted to a family requirement instead of becoming a user override.


Suggested files/modules/symbols to inspect

Core sourceport compatibility:

- `crates/caco-core/src/sourceports.rs`
  - `SourceportFamily`
  - `FAMILIES`
  - `identify_family`
  - `family_name`
  - `detect_sourceports`

Config:

- `crates/caco-core/src/config.rs`
  - `Config`
  - `get_default_sourceport`
  - `get_zdoom_sourceport`
  - `resolve_sourceport`

Launcher:

- `crates/caco-core/src/player.rs`
  - `play`
  - sourceport selection logic
  - existing `zdoom_required` logic

Database/schema/model:

- `crates/caco-core/src/db/schema.rs`
  - migration registry
  - `wads` table
  - migration helpers
- `crates/caco-core/src/db/models.rs`
  - `WadRecord`
  - `WadRecord::from_row`
  - `ALLOWED_UPDATE_FIELDS`

Import/enrichment:

- `crates/caco-sources/src/import_service.rs`
  - Doomworld import path
  - `auto_link_zdoom_required`
  - `auto_link_complevel`
  - `is_persistable_port`
- `crates/caco-sources/src/doomworld/parser.rs`
  - sourceport detection / normalization

UI/output likely needing display updates:

- `crates/caco-cli/src/output.rs`
- `crates/caco-tui/src/screens/wad_detail.rs`
- `crates/caco-tui/src/screens/wad_edit.rs`
- `crates/caco-gui/src/dialogs/edit.rs`
- any CLI modify command that exposes `custom_sourceport`


Implementation requirements

1. Add a nullable `required_sourceport_family TEXT` column to the `wads` table.

   - Add a new migration after the current latest migration.
   - Use the existing migration style in `schema.rs`.
   - Add the column idempotently with `add_column_if_missing`.
   - Update fresh-schema creation if appropriate.
   - Update schema tests that assert expected WAD columns.

2. Update `WadRecord`.

   - Add `required_sourceport_family: Option<String>`.
   - Populate it in `WadRecord::from_row`.
   - Add it to `ALLOWED_UPDATE_FIELDS`.

3. Preserve `custom_sourceport` as a user override only.

   - Do not write automatically detected Doomworld/Doom Wiki sourceport metadata into `custom_sourceport`.
   - In `ImportService`, when a detected sourceport is found, convert it to a family using `caco_core::sourceports::family_name`.
   - Store the family name in `required_sourceport_family` if the WAD does not already have one.
   - Example: detected `dsda-doom` or `nyan-doom` should store `required_sourceport_family = "dsda"`.
   - Example: detected `gzdoom`, `uzdoom`, `lzdoom`, etc. should store `required_sourceport_family = "zdoom"`.

4. Keep `zdoom_required` compatibility for now.

   - Do not remove `zdoom_required`.
   - For now, if existing logic sets or reads `zdoom_required`, preserve it.
   - When a WAD is detected as ZDoom-required, it is acceptable to also set `required_sourceport_family = "zdoom"` when empty.
   - Avoid turning this task into a full migration/removal of `zdoom_required`.

5. Add sourceport-family-aware launch resolution.

   In `player::play`, replace the simple sourceport selection with a helper or clear inline logic:

   Desired precedence:

   - CLI `opts.sourceport`: absolute override.
   - WAD `custom_sourceport`: user override.
   - WAD `required_sourceport_family`: choose a compatible sourceport.
   - Global default sourceport.

   Family resolution should work like this:

   - If the global default sourceport belongs to the required family, use it.
     - Example: global default `nyan-doom`, required family `dsda` => use `nyan-doom`.
   - Otherwise, if required family is `zdoom`, use existing `config::get_zdoom_sourceport()`.
   - Otherwise, choose a reasonable installed executable from that family if available.
     - Prefer user-configured family preferences if implemented.
     - Otherwise, use `sourceports::detect_sourceports()` and pick the first installed executable matching the family.
   - As a final fallback, use the first known executable for that family from `sourceports::FAMILIES`.
   - If no family resolution is possible, fall back to the global default.

   Important examples:

   - `sourceport = "nyan-doom"`, `required_sourceport_family = "dsda"`, no `custom_sourceport`
     => launch `nyan-doom`
   - `sourceport = "nyan-doom"`, `custom_sourceport = "dsda-doom"`
     => launch `dsda-doom`
   - `sourceport = "nyan-doom"`, `required_sourceport_family = "zdoom"`
     => launch configured ZDoom sourceport, e.g. `uzdoom` or `gzdoom`
   - CLI `--sourceport woof`
     => launch `woof`, regardless of metadata

6. Consider adding config support for family preferences.

   This can be either included now or left as a follow-up, but the design should not block it.

   Possible config shape:

   ```toml
   [sourceport_preferences]
   dsda = "nyan-doom"
   zdoom = "uzdoom"
   chocolate = "crispy-doom"
````

If implemented:

* Add `sourceport_preferences: HashMap<String, String>` to `Config`.
* Update defaults.
* Update `config.example.toml`.
* Use this before installed-port fallback when resolving `required_sourceport_family`.

If not implemented:

* Keep the resolver simple: global default if compatible, zdoom special case, installed family member, known family executable.

7. Update output/UI wording.

   Anywhere `custom_sourceport` is displayed as sourceport metadata, make sure the distinction is clear:

   * `custom_sourceport`: “Sourceport override” or “Custom sourceport”
   * `required_sourceport_family`: “Required sourceport family” or “Compatibility family”

   Avoid showing automatically detected family metadata as if it were a user override.

8. Add tests.

   Add or update tests for:

   * Migration adds `required_sourceport_family`.
   * `WadRecord::from_row` reads it.
   * `ALLOWED_UPDATE_FIELDS` includes it.
   * Importing or auto-linking a detected `dsda-doom` stores `required_sourceport_family = "dsda"` and does not set `custom_sourceport`.
   * Launch resolution:

     * default `nyan-doom` satisfies required family `dsda`
     * WAD `custom_sourceport` still beats required family
     * CLI sourceport still beats everything
     * zdoom-required/family WAD uses configured zdoom sourceport when default is not zdoom-family

Constraints

* Do not remove `zdoom_required` in this task.
* Do not make imported metadata overwrite user choices.
* Do not treat weak imported sourceport recommendations as stronger than explicit user config.
* Preserve existing behavior for WADs with manually set `custom_sourceport`.
* Keep the change backwards compatible with existing databases.
* Avoid broad UI redesign; only adjust labels/fields needed for correctness.

Risks and edge cases

* Existing databases may already have `custom_sourceport = "dsda-doom"` from previous auto-import behavior. Do not automatically migrate these unless there is a reliable way to distinguish imported values from user-set values.
* Some ports in `is_persistable_port`, such as `cherry-doom`, `edge`, `doomsday`, `zandronum`, `odamex`, and `3dge`, may not currently exist in `sourceports::FAMILIES`. For unknown families, either do not set `required_sourceport_family`, or add family definitions deliberately.
* Doomworld sourceport detection can include “tested with” rather than “required.” Store it as a family compatibility hint, but do not let it override user defaults when the default belongs to that family.
* Be careful resolving sourceport family after `config::resolve_sourceport`, because full paths must still identify by basename.
* If a sourceport is specified as a full path, `sourceports::family_name` / `identify_family` should still work by basename.

Validation steps

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```

Manual validation scenarios:

1. Configure:

```toml
sourceport = "nyan-doom"
```

Import or edit a WAD so it has:

```text
required_sourceport_family = "dsda"
custom_sourceport = NULL
```

Playing it should launch `nyan-doom`.

2. Set the same WAD to:

```text
custom_sourceport = "dsda-doom"
required_sourceport_family = "dsda"
```

Playing it should launch `dsda-doom`.

3. Configure:

```toml
sourceport = "nyan-doom"
zdoom_sourceport = "uzdoom"
```

For a WAD with:

```text
required_sourceport_family = "zdoom"
```

Playing it should launch `uzdoom`.

4. Run with CLI override:

```bash
caco play <id> --sourceport woof
```

It should launch `woof` regardless of `required_sourceport_family`.

5. Import a Doomworld thread that mentions DSDA-Doom. Confirm the resulting WAD stores the compatibility family, not a user sourceport override.

```
