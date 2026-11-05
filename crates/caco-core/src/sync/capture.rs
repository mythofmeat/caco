//! What travels between machines, and how it gets into the log.
//!
//! [`ENTITIES`] is the one description of which tables and columns are history. The
//! triggers that notice a write, the rows logged when a library first gets a log, and
//! the merge that applies another machine's events are all generated from it, so they
//! cannot disagree about what a field is called or what it means.
//!
//! # Why triggers
//!
//! Sessions, completions and status changes are written from all over: `db/`, raw SQL in
//! `player.rs`, the dialogs. Adding a call to each would only last until the next write
//! path forgot. A stored `availability` column once drifted for months for that reason:
//! `clear_cached_path` was the one writer that went around whatever kept it honest. A
//! trigger cannot be gone around, by a new write path, by a test, or by someone in a
//! `sqlite3` prompt, and it joins the transaction of the write it records, so a write and
//! its record of having happened are committed or rolled back together.
//!
//! They use nothing beyond SQLite's own functions, unlike what reads a mirror's rows, or
//! `library.db` could not be written to by anything but caco.
//!
//! # What does not travel
//!
//! Anything true of only one machine: where a file is (`cached_path`, `file_mtime`), and
//! a sourceport or IWAD named by a path, which is what [`Role::Name`] leaves behind.
//! Anything that can be worked out again from a file or a dataset also stays:
//! `wad_analysis`, `wad_files`, what the mirrors hold, the facts about a Cacoward.
//! `LOCAL_COLUMNS` and `LOCAL_TABLES` list all of it, and a test fails on a column that is
//! on neither side, so adding one means deciding.

use rusqlite::Connection;

use crate::Result;

/// What a column holds, as far as the merge is concerned. Checked on the way in, because
/// a `WadRecord` cannot be read from a row whose `year` is text, and one damaged line
/// must not be able to take the library down with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Text,
    Int,
}

/// A row some other row points at, by a rowid that means nothing on another machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent {
    pub table: &'static str,
    /// The kind of event this row's own history goes by.
    pub kind: &'static str,
    /// The column that names it on every machine.
    pub global: &'static str,
}

pub const WAD: Parent = Parent {
    table: "wads",
    kind: "wad",
    global: "uid",
};
pub const PLAYTHROUGH: Parent = Parent {
    table: "playthroughs",
    kind: "playthrough",
    global: "uid",
};
pub const COMPANION: Parent = Parent {
    table: "companion_files_registry",
    kind: "companion",
    global: "md5",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Travels as it is.
    Plain,
    /// A name, which travels, or a path, which is true of one machine and stays there:
    /// `custom_iwad` holds `doom2` or `/home/me/iwads/doom2.wad`. The other side refuses
    /// a path too, because these end up in a file name or a launch.
    Name,
    /// One of the columns that together say what a file is, which only ever travel
    /// together: see [`super::merge`] for what a machine with its own file does with them.
    Identity,
    /// Set when the row is made, by the row's trigger if not by the insert, and never
    /// changed by anything but a merge.
    Assigned,
    /// The row this one belongs to, for its whole life, so only an `add` says which.
    Owner(Parent),
    /// A rowid in this table, and the row's global name in the log. `None` is a value a
    /// reference can have, which is how a Cacoward is unlinked.
    Ref(Parent),
    /// Part of what makes a row this row, where nothing else names it.
    KeyPart,
    /// Both of the above.
    KeyRef(Parent),
}

/// One thing a row says.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// What the log calls it, which is forever: see [`super::event`].
    pub name: &'static str,
    pub column: &'static str,
    pub ty: Ty,
    pub role: Role,
    /// A row cannot be made without it.
    pub required: bool,
    /// The only values it can have.
    pub one_of: Option<&'static [&'static str]>,
    /// What a new row has here without anyone having chosen it, which is left out of its
    /// `add`. Otherwise a second import of a WAD would have an opinion on its status.
    pub unset: Option<Unset>,
}

/// See [`Field::unset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unset {
    Text(&'static str),
    Int(i64),
}

impl Field {
    const fn new(name: &'static str, ty: Ty) -> Self {
        Self {
            name,
            column: name,
            ty,
            role: Role::Plain,
            required: false,
            one_of: None,
            unset: None,
        }
    }

    const fn text(name: &'static str) -> Self {
        Self::new(name, Ty::Text)
    }

    const fn int(name: &'static str) -> Self {
        Self::new(name, Ty::Int)
    }

    const fn name_not_path(name: &'static str) -> Self {
        Self::text(name).role(Role::Name)
    }

    /// A rowid column, which the log knows by what it points at.
    const fn reference(name: &'static str, column: &'static str, parent: Parent) -> Self {
        let mut field = Self::text(name).role(Role::Ref(parent));
        field.column = column;
        field
    }

    const fn owner(name: &'static str, column: &'static str, parent: Parent) -> Self {
        let mut field = Self::text(name).role(Role::Owner(parent));
        field.column = column;
        field.required = true;
        field
    }

    const fn key_ref(name: &'static str, column: &'static str, parent: Parent) -> Self {
        let mut field = Self::text(name).role(Role::KeyRef(parent));
        field.column = column;
        field.required = true;
        field
    }

    const fn key_part(name: &'static str) -> Self {
        let mut field = Self::text(name).role(Role::KeyPart);
        field.required = true;
        field
    }

    const fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    const fn one_of(mut self, values: &'static [&'static str]) -> Self {
        self.one_of = Some(values);
        self
    }

    const fn unset(mut self, value: Unset) -> Self {
        self.unset = Some(value);
        self
    }

    /// Whether a value of this field names something, as opposed to saying something.
    pub fn is_key(&self) -> bool {
        matches!(self.role, Role::KeyPart | Role::KeyRef(_))
    }

    /// Whether an update to this column is something to write down.
    fn captured_on_update(&self) -> bool {
        !matches!(self.role, Role::Assigned | Role::Owner(_))
    }

    /// What this field points at, if it is a rowid here and a name in the log.
    pub fn parent(&self) -> Option<Parent> {
        match self.role {
            Role::Owner(parent) | Role::Ref(parent) | Role::KeyRef(parent) => Some(parent),
            _ => None,
        }
    }
}

/// What a row's existence goes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One row per uid. An `add` makes it, a `set` changes it, and a `delete` is final:
    /// a WAD deleted for good stays deleted on every machine, whichever order they hear
    /// about it in, and importing it again makes a new row with a new uid.
    Row,
    /// Something that can be there, not be, and be there again: a tag, say. Its `present`
    /// field goes by last writer wins like any other, so taking a tag off on one machine
    /// and putting it back on another is settled the same way everywhere.
    Membership,
    /// Rows that exist anyway, for some other reason, which the log only has an opinion
    /// about part of: a Cacoward's row comes from the dataset, its link from the user.
    Annotation,
}

/// What names a row in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// This column, which is the same on every machine.
    Column(&'static str),
    /// Its [`Role::KeyPart`]s and [`Role::KeyRef`]s, joined.
    Parts,
}

/// A kind of thing whose history is in the log.
#[derive(Debug, Clone, Copy)]
pub struct Entity {
    pub kind: &'static str,
    pub table: &'static str,
    pub shape: Shape,
    pub key: Key,
    pub fields: &'static [Field],
    /// When the row last changed, as far as a row that predates the log can say, which
    /// orders it against the same row logged by another machine.
    as_of: &'static str,
    /// Which rows predating the log have anything to say.
    baseline_where: &'static str,
}

impl Entity {
    pub fn field(&self, name: &str) -> Option<&'static Field> {
        self.fields.iter().find(|field| field.name == name)
    }
}

pub const STATUSES: &[&str] = &["unplayed", "in-progress", "completed", "abandoned"];
pub const SOURCE_TYPES: &[&str] = &["idgames", "doomwiki", "doomworld", "url", "local"];

/// Every kind of thing that travels. Parents before children, since that is the order a
/// log is first written in, and an event with no parent yet has to wait for one.
pub static ENTITIES: &[Entity] = &[
    Entity {
        kind: "wad",
        table: "wads",
        shape: Shape::Row,
        key: Key::Column("uid"),
        as_of: "COALESCE(t.updated_at, t.created_at)",
        baseline_where: "1",
        fields: &[
            Field::text("title").required(),
            Field::text("source_type")
                .role(Role::Assigned)
                .required()
                .one_of(SOURCE_TYPES),
            Field::text("created_at").role(Role::Assigned),
            Field::text("data_key").role(Role::Assigned),
            Field::text("author"),
            Field::int("year"),
            Field::text("description"),
            Field::text("status")
                .one_of(STATUSES)
                .unset(Unset::Text("unplayed")),
            Field::int("rating"),
            Field::text("notes"),
            Field::text("source_id"),
            Field::text("source_url"),
            Field::text("idgames_id"),
            Field::name_not_path("filename"),
            Field::text("version"),
            Field::name_not_path("custom_iwad"),
            Field::name_not_path("custom_sourceport"),
            Field::text("required_sourceport_family"),
            Field::text("custom_args"),
            Field::name_not_path("custom_config"),
            Field::int("complevel"),
            Field::int("zdoom_required"),
            Field::text("download_urls"),
            Field::text("stats_snapshot"),
            Field::int("gc_ignore").unset(Unset::Int(0)),
            Field::text("deleted_at"),
            Field::int("file_size").role(Role::Identity),
            Field::text("file_sha256").role(Role::Identity),
            Field::text("file_sha1").role(Role::Identity),
            Field::text("content_sha256").role(Role::Identity),
        ],
    },
    Entity {
        kind: "playthrough",
        table: "playthroughs",
        shape: Shape::Row,
        key: Key::Column("uid"),
        as_of: "COALESCE(t.completed_at, t.started_at)",
        baseline_where: "1",
        fields: &[
            Field::owner("wad", "wad_id", WAD),
            Field::text("started_at").required(),
            Field::text("completed_at"),
            Field::text("stats_snapshot"),
            Field::text("notes"),
            Field::text("created_at").role(Role::Assigned),
        ],
    },
    Entity {
        kind: "session",
        table: "sessions",
        shape: Shape::Row,
        key: Key::Column("uid"),
        as_of: "COALESCE(t.ended_at, t.started_at)",
        baseline_where: "1",
        fields: &[
            Field::owner("wad", "wad_id", WAD),
            Field::text("started_at").required(),
            Field::text("ended_at"),
            Field::int("duration_seconds"),
            Field::text("sourceport"),
            Field::text("notes"),
            Field::int("exit_code"),
            Field::text("stats_before"),
            Field::text("stats_after"),
            Field::text("content_sha256"),
            Field::reference("playthrough", "playthrough_id", PLAYTHROUGH),
        ],
    },
    Entity {
        kind: "completion",
        table: "wad_completions",
        shape: Shape::Row,
        key: Key::Column("uid"),
        as_of: "t.completed_at",
        baseline_where: "1",
        fields: &[
            Field::owner("wad", "wad_id", WAD),
            Field::text("completed_at"),
            Field::text("stats_snapshot"),
            Field::text("notes"),
        ],
    },
    Entity {
        kind: "tag",
        table: "tags",
        shape: Shape::Membership,
        key: Key::Parts,
        as_of: "(SELECT created_at FROM wads WHERE id = t.wad_id)",
        baseline_where: "1",
        fields: &[Field::key_ref("wad", "wad_id", WAD), Field::key_part("tag")],
    },
    Entity {
        kind: "companion",
        table: "companion_files_registry",
        shape: Shape::Membership,
        key: Key::Column("md5"),
        as_of: "NULL",
        baseline_where: "1",
        fields: &[
            Field::name_not_path("filename").required(),
            Field::int("size").required(),
        ],
    },
    Entity {
        kind: "wad_companion",
        table: "wad_companions",
        shape: Shape::Membership,
        key: Key::Parts,
        as_of: "NULL",
        baseline_where: "1",
        fields: &[
            Field::key_ref("wad", "wad_id", WAD),
            Field::key_ref("companion", "companion_id", COMPANION),
            Field::int("enabled"),
            Field::int("load_order"),
        ],
    },
    Entity {
        kind: "collection",
        table: "smart_collections",
        shape: Shape::Membership,
        key: Key::Column("name"),
        as_of: "t.created_at",
        baseline_where: "1",
        fields: &[
            Field::text("query").required(),
            Field::text("sort_by"),
            Field::int("sort_desc"),
            Field::text("created_at").role(Role::Assigned),
        ],
    },
    Entity {
        kind: "cacoward",
        table: "cacowards",
        shape: Shape::Annotation,
        key: Key::Column("key"),
        as_of: "t.updated_at",
        baseline_where: "t.wad_id IS NOT NULL OR t.manual_override <> 0 \
                         OR t.supported_override IS NOT NULL",
        fields: &[
            Field::reference("wad", "wad_id", WAD),
            Field::int("manual_override"),
            Field::int("supported_override"),
        ],
    },
    Entity {
        kind: "saves",
        table: "wad_saves",
        shape: Shape::Membership,
        key: Key::Parts,
        as_of: "t.recorded_at",
        baseline_where: "1",
        fields: &[
            Field::key_ref("wad", "wad_id", WAD),
            Field::key_part("machine"),
            Field::text("files").required(),
            Field::text("recorded_at").required(),
        ],
    },
];

/// Columns of the tables above that are true of one machine, or can be worked out again.
/// Each table's rowid is here because it is the reason for all of this.
///
/// Nothing but a test reads these, or this one. They are what a test holds every column
/// to, so that adding one means deciding which list it goes on.
#[cfg(test)]
const LOCAL_COLUMNS: &[(&str, &[&str])] = &[
    (
        "wads",
        &[
            "id",
            "cached_path",
            "last_known_path",
            "file_mtime",
            "updated_at",
        ],
    ),
    ("playthroughs", &["id"]),
    ("sessions", &["id"]),
    ("wad_completions", &["id"]),
    ("tags", &["id"]),
    ("companion_files_registry", &["id", "path"]),
    ("wad_companions", &[]),
    ("smart_collections", &["id"]),
    (
        "cacowards",
        &[
            "id",
            "year",
            "category",
            "page_order",
            "wad_title",
            "wad_author",
            "idgames_id",
            "doomworld_id",
            "doomwiki_page",
            "sha256",
            "supported_default",
            "created_at",
            "updated_at",
        ],
    ),
    ("wad_saves", &[]),
];

/// Tables with nothing in them that belongs in a log.
#[cfg(test)]
const LOCAL_TABLES: &[&str] = &[
    // The database's own bookkeeping.
    "schema_migrations",
    "sync_outbox",
    "sync_clocks",
    "sync_seen",
    "sync_parked",
    "sync_files",
    "sync_tombstones",
    "sync_machines",
    "sync_state",
    "sync_applying",
    // What each machine's IWADs are, which are found by path.
    "iwads",
    "id24_wads",
    // Worked out again from the file.
    "wad_analysis",
    "wad_files",
    // Worked out again from what each mirror's index says.
    "mirror_files",
    "mirror_file_cacowards",
    // Worked out again from the log.
    "wad_stats_by_machine",
];

/// The tables the log needs, for the migration that adds them.
pub(crate) const SYNC_TABLES: &str = "
    CREATE TABLE IF NOT EXISTS sync_outbox (
        seq INTEGER PRIMARY KEY,
        id TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))),
        at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        op TEXT NOT NULL,
        kind TEXT NOT NULL,
        key TEXT NOT NULL,
        fields TEXT,
        as_of INTEGER NOT NULL DEFAULT 0,
        event TEXT,
        clock TEXT
    );
    CREATE TABLE IF NOT EXISTS sync_clocks (
        entity TEXT NOT NULL,
        field TEXT NOT NULL,
        clock TEXT NOT NULL,
        PRIMARY KEY (entity, field)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_seen (id TEXT PRIMARY KEY) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_parked (
        id TEXT PRIMARY KEY,
        clock TEXT NOT NULL,
        line TEXT NOT NULL,
        reason TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_files (
        path TEXT PRIMARY KEY,
        size INTEGER NOT NULL,
        mtime_ms INTEGER
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_tombstones (
        kind TEXT NOT NULL,
        key TEXT NOT NULL,
        PRIMARY KEY (kind, key)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_machines (
        machine TEXT PRIMARY KEY,
        last_at TEXT NOT NULL,
        events INTEGER NOT NULL DEFAULT 0
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_state (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS sync_applying (flag INTEGER);
    CREATE TABLE IF NOT EXISTS wad_saves (
        wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
        machine TEXT NOT NULL,
        files TEXT NOT NULL,
        recorded_at TEXT NOT NULL,
        PRIMARY KEY (wad_id, machine)
    );
    CREATE TABLE IF NOT EXISTS wad_stats_by_machine (
        wad_id INTEGER NOT NULL REFERENCES wads(id) ON DELETE CASCADE,
        machine TEXT NOT NULL,
        is_this_machine INTEGER NOT NULL DEFAULT 0,
        clock TEXT NOT NULL,
        stats TEXT NOT NULL,
        PRIMARY KEY (wad_id, machine)
    );
";

/// Whether a trigger should write anything down. A merge's own writes are other machines'
/// events being applied, and writing them down again would have the machines echoing
/// each other for ever. The flag is set and cleared inside the merge's transaction, so
/// no other connection ever sees it.
const NOT_APPLYING: &str = "NOT EXISTS (SELECT 1 FROM sync_applying)";

/// A new row's uid: what its source calls it, if it has a source that names things, so
/// that the same WAD imported on two machines is one WAD and not two. A second import
/// forced past the duplicate check gets a new uid, and so does anything imported again
/// after it was deleted for good, since a `delete` is final for that uid.
///
/// As `find_duplicate` compares it, and only digits, or this would make a folder name
/// out of whatever a row happened to hold. This is also why the trigger needs nothing
/// beyond SQLite's own functions: see the module docs.
fn new_wad_uid() -> String {
    format!(
        "CASE WHEN NEW.source_type IN ('idgames', 'doomwiki', 'doomworld') \
           AND COALESCE(NEW.source_id, '') <> '' \
           AND NEW.source_id NOT GLOB '*[^0-9]*' \
           AND NOT EXISTS (SELECT 1 FROM wads \
               WHERE uid = NEW.source_type || '-' || NEW.source_id) \
           AND NOT EXISTS (SELECT 1 FROM sync_tombstones \
               WHERE kind = 'wad' AND key = NEW.source_type || '-' || NEW.source_id) \
         THEN NEW.source_type || '-' || NEW.source_id \
         ELSE {RANDOM_UID} END"
    )
}

const RANDOM_UID: &str = "lower(hex(randomblob(16)))";

/// What a field's value is, in terms of a row called `row`.
fn value_sql(entity: &Entity, field: &Field, row: &str) -> String {
    let column = field.column;
    if let Some(parent) = field.parent() {
        return format!(
            "(SELECT {global} FROM {table} WHERE id = {row}.{column})",
            global = parent.global,
            table = parent.table
        );
    }
    match field.role {
        Role::Assigned if row == "NEW" => format!(
            "(SELECT {column} FROM {table} WHERE id = NEW.id)",
            table = entity.table
        ),
        Role::Name => format!(
            "CASE WHEN {} THEN NULL ELSE {row}.{column} END",
            is_path_sql(&format!("{row}.{column}"))
        ),
        _ => format!("{row}.{column}"),
    }
}

/// Whether text that should be a name is a path. Never true of NULL.
fn is_path_sql(value: &str) -> String {
    format!("(instr({value}, '/') > 0 OR instr({value}, char(92)) > 0)")
}

/// What names a row called `row` in the log. NULL if a parent has gone, which is how a
/// row that went with its parent says so: see [`delete_trigger`].
fn key_sql(entity: &Entity, row: &str) -> String {
    match entity.key {
        Key::Column(column) if column == "uid" && row == "NEW" => format!(
            "(SELECT uid FROM {table} WHERE id = NEW.id)",
            table = entity.table
        ),
        Key::Column(column) => format!("{row}.{column}"),
        Key::Parts => entity
            .fields
            .iter()
            .filter(|field| field.is_key())
            .map(|field| value_sql(entity, field, row))
            .collect::<Vec<_>>()
            .join(" || '/' || "),
    }
}

/// `json_object` of a whole row. A membership says it is there, or `present` would never
/// be written by anything but taking one away.
fn all_fields_json(entity: &Entity, row: &str) -> String {
    let mut parts: Vec<String> = entity
        .fields
        .iter()
        .map(|field| format!("'{}', {}", field.name, value_sql(entity, field, row)))
        .collect();
    if entity.shape == Shape::Membership {
        parts.push("'present', 1".to_string());
    }
    format!("json_object({})", parts.join(", "))
}

/// Whether an update changed what this field says. A name changed to a path is not
/// something another machine can use, so it keeps what it had.
fn changed_sql(entity: &Entity, field: &Field) -> String {
    let column = field.column;
    match field.role {
        Role::Identity => entity
            .fields
            .iter()
            .filter(|other| other.role == Role::Identity)
            .map(|other| format!("OLD.{0} IS NOT NEW.{0}", other.column))
            .collect::<Vec<_>>()
            .join(" OR "),
        Role::Name => format!(
            "OLD.{column} IS NOT NEW.{column} AND COALESCE({}, 0) = 0",
            is_path_sql(&format!("NEW.{column}"))
        ),
        _ => format!("OLD.{column} IS NOT NEW.{column}"),
    }
}

fn insert_trigger(entity: &Entity) -> Option<String> {
    let (table, kind) = (entity.table, entity.kind);
    let key = key_sql(entity, "NEW");
    let fields = all_fields_json(entity, "NEW");
    match entity.shape {
        Shape::Row => {
            let uid = if table == "wads" {
                new_wad_uid()
            } else {
                RANDOM_UID.to_string()
            };
            let data_key = if table == "wads" {
                "UPDATE wads SET data_key = uid WHERE id = NEW.id AND data_key IS NULL;"
            } else {
                ""
            };
            Some(format!(
                "CREATE TRIGGER sync_{table}_insert AFTER INSERT ON {table} \
                 BEGIN \
                 UPDATE {table} SET uid = {uid} WHERE id = NEW.id AND uid IS NULL; \
                 {data_key} \
                 INSERT INTO sync_outbox (op, kind, key, fields) \
                 SELECT 'add', '{kind}', {key}, {fields} WHERE {NOT_APPLYING}; \
                 END"
            ))
        }
        Shape::Membership => Some(format!(
            "CREATE TRIGGER sync_{table}_insert AFTER INSERT ON {table} \
             WHEN {NOT_APPLYING} \
             BEGIN \
             INSERT INTO sync_outbox (op, kind, key, fields) \
             SELECT 'set', '{kind}', {key}, {fields} WHERE {key} IS NOT NULL; \
             END"
        )),
        Shape::Annotation => None,
    }
}

fn update_trigger(entity: &Entity) -> Option<String> {
    let (table, kind) = (entity.table, entity.kind);
    let captured: Vec<&Field> = entity
        .fields
        .iter()
        .filter(|field| !field.is_key() && field.captured_on_update())
        .collect();
    if captured.is_empty() {
        return None;
    }
    let any_changed = captured
        .iter()
        .map(|field| format!("({})", changed_sql(entity, field)))
        .collect::<Vec<_>>()
        .join(" OR ");
    // What names the row goes in every event, since nothing parses a key apart.
    let mut selects: Vec<String> = entity
        .fields
        .iter()
        .filter(|field| field.is_key())
        .map(|field| {
            format!(
                "SELECT '{}' AS f, {} AS v",
                field.name,
                value_sql(entity, field, "NEW")
            )
        })
        .collect();
    selects.extend(captured.iter().map(|field| {
        format!(
            "SELECT '{}' AS f, {} AS v WHERE {}",
            field.name,
            value_sql(entity, field, "NEW"),
            changed_sql(entity, field)
        )
    }));
    // The key is checked in WHEN and not in a WHERE on the insert: an aggregate returns a
    // row even when no row gets through a WHERE, and its NULL key would then fail the
    // NOT NULL on `sync_outbox`, and take the user's own write down with it.
    let key = key_sql(entity, "NEW");
    Some(format!(
        "CREATE TRIGGER sync_{table}_update AFTER UPDATE ON {table} \
         WHEN ({any_changed}) AND {NOT_APPLYING} AND {key} IS NOT NULL \
         BEGIN \
         INSERT INTO sync_outbox (op, kind, key, fields) \
         SELECT 'set', '{kind}', {key}, json_group_object(f, v) FROM ({}); \
         END",
        selects.join(" UNION ALL ")
    ))
}

/// A row going, which is a fact worth keeping on this machine however it went, and an
/// event only if someone did it: a row that went with its parent says nothing of its own,
/// since its parent's event says it all. Its key is NULL by then, or for a row, its
/// parent's row is gone.
fn delete_trigger(entity: &Entity) -> Option<String> {
    let (table, kind) = (entity.table, entity.kind);
    match entity.shape {
        Shape::Row => {
            let parent_alive = entity
                .fields
                .iter()
                .find_map(|field| match field.role {
                    Role::Owner(parent) => Some(format!(
                        "AND EXISTS (SELECT 1 FROM {} WHERE id = OLD.{})",
                        parent.table, field.column
                    )),
                    _ => None,
                })
                .unwrap_or_default();
            Some(format!(
                "CREATE TRIGGER sync_{table}_delete AFTER DELETE ON {table} \
                 BEGIN \
                 INSERT OR IGNORE INTO sync_tombstones (kind, key) VALUES ('{kind}', OLD.uid); \
                 INSERT INTO sync_outbox (op, kind, key) \
                 SELECT 'delete', '{kind}', OLD.uid \
                 WHERE OLD.uid IS NOT NULL AND {NOT_APPLYING} {parent_alive}; \
                 END"
            ))
        }
        Shape::Membership => {
            let key = key_sql(entity, "OLD");
            let mut parts: Vec<String> = entity
                .fields
                .iter()
                .filter(|field| field.is_key())
                .map(|field| format!("'{}', {}", field.name, value_sql(entity, field, "OLD")))
                .collect();
            parts.push("'present', 0".to_string());
            Some(format!(
                "CREATE TRIGGER sync_{table}_delete AFTER DELETE ON {table} \
                 WHEN {NOT_APPLYING} \
                 BEGIN \
                 INSERT INTO sync_outbox (op, kind, key, fields) \
                 SELECT 'set', '{kind}', {key}, json_object({}) WHERE {key} IS NOT NULL; \
                 END",
                parts.join(", ")
            ))
        }
        Shape::Annotation => None,
    }
}

/// Clearing a WAD's snapshot is starting over (a new playthrough, a clean), which takes
/// what other machines said about the last attempt with it, at once: if it waited for
/// the next flush, a session ending before then would merge it straight back. Not during
/// a merge, which works out what is left for itself: whatever was said after a clear it
/// has only just heard about stays.
const STATS_RESET_TRIGGER: &str = "CREATE TRIGGER sync_wads_stats_reset \
     AFTER UPDATE OF stats_snapshot ON wads \
     WHEN NEW.stats_snapshot IS NULL AND NOT EXISTS (SELECT 1 FROM sync_applying) \
     BEGIN DELETE FROM wad_stats_by_machine WHERE wad_id = NEW.id; END";

/// Every trigger there should be, as `(name, CREATE statement)`.
fn triggers() -> Vec<(String, String)> {
    let mut statements: Vec<String> = ENTITIES
        .iter()
        .flat_map(|entity| {
            [
                insert_trigger(entity),
                update_trigger(entity),
                delete_trigger(entity),
            ]
        })
        .flatten()
        .collect();
    statements.push(STATS_RESET_TRIGGER.to_string());
    statements
        .into_iter()
        .map(|statement| {
            let name = statement
                .split_whitespace()
                .nth(2)
                .unwrap_or_default()
                .to_string();
            (name, statement)
        })
        .collect()
}

/// The triggers in this database, as `(name, CREATE statement)`, by name.
fn installed_triggers(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(
        "SELECT name, sql FROM sqlite_master \
         WHERE type = 'trigger' AND name LIKE 'sync\\_%' ESCAPE '\\' ORDER BY name",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Make the triggers what this build says they should be.
///
/// Compares against the text of what is there rather than a version number, so that
/// nobody can change [`ENTITIES`] and forget to bump something. Reads and goes away if
/// they agree, which is every launch but one.
pub(crate) fn install(conn: &Connection) -> Result<()> {
    let mut wanted = triggers();
    wanted.sort();
    if installed_triggers(conn)? == wanted {
        return Ok(());
    }
    crate::db::with_transaction(conn, |tx| {
        uninstall(tx)?;
        for (_, statement) in &wanted {
            tx.execute_batch(statement)?;
        }
        Ok(())
    })
}

/// Take the triggers out, because a migration's rewrites are not anyone's edits, and
/// every machine will run the same one. They also keep `ALTER TABLE DROP COLUMN` from
/// working on a column they mention.
pub(crate) fn uninstall(conn: &Connection) -> Result<()> {
    for (name, _) in installed_triggers(conn)? {
        conn.execute_batch(&format!("DROP TRIGGER {name}"))?;
    }
    Ok(())
}

/// Log every row that is already here, when a library first gets a log.
///
/// As an `add` at the time the row last changed, which is what makes this safe on two
/// machines that each have a copy of the same library. They log the same rows with the
/// same uids and it comes to nothing, and where they differ the more recent edit wins,
/// and everything after this beats all of it.
pub(crate) fn log_existing_rows(conn: &Connection) -> Result<()> {
    for entity in ENTITIES {
        let table = entity.table;
        let kind = entity.kind;
        let key = key_sql(entity, "t");
        let fields = all_fields_json(entity, "t");
        let as_of = entity.as_of;
        let condition = entity.baseline_where;
        conn.execute_batch(&format!(
            "INSERT INTO sync_outbox (op, kind, key, fields, at, as_of) \
             SELECT 'add', '{kind}', {key}, {fields}, COALESCE({as_of}, '1970-01-01T00:00:00Z'), 1 \
             FROM {table} AS t WHERE {condition}"
        ))?;
    }
    Ok(())
}

/// The entity behind an event's `kind`, if this build knows of it.
pub fn entity(kind: &str) -> Option<&'static Entity> {
    ENTITIES.iter().find(|entity| entity.kind == kind)
}

/// For comparing whole libraries in tests: what names a row of `t`.
#[cfg(test)]
pub(super) fn test_key_sql(entity: &Entity) -> String {
    key_sql(entity, "t")
}

/// For comparing whole libraries in tests: everything a row of `t` says.
#[cfg(test)]
pub(super) fn test_fields_sql(entity: &Entity) -> String {
    all_fields_json(entity, "t")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::db::{self, NewWad, SourceType, WadUpdate};
    use serde_json::{Value, json};

    fn setup() -> Connection {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        conn
    }

    /// Everything waiting to go out, as `(op, kind, key, fields)`.
    fn outbox(conn: &Connection) -> Vec<(String, String, String, Value)> {
        let mut statement = conn
            .prepare("SELECT op, kind, key, fields FROM sync_outbox ORDER BY seq")
            .unwrap();
        statement
            .query_map([], |row| {
                let fields: Option<String> = row.get(3)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    fields
                        .map(|text| serde_json::from_str(&text).unwrap())
                        .unwrap_or(Value::Null),
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn clear(conn: &Connection) {
        conn.execute("DELETE FROM sync_outbox", []).unwrap();
    }

    fn add_wad(conn: &Connection) -> i64 {
        db::add_wad(conn, &NewWad::new("Test", SourceType::Local)).unwrap()
    }

    fn uid_of(conn: &Connection, wad: i64) -> String {
        conn.query_row("SELECT uid FROM wads WHERE id = ?1", [wad], |row| {
            row.get(0)
        })
        .unwrap()
    }

    /// Adding a column means deciding which side of the line it is on. Without this a
    /// new column would quietly not travel, which is only noticed once it has been lost.
    #[test]
    fn test_every_column_of_every_table_has_been_classified() {
        let conn = setup();
        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();

        for table in &tables {
            if LOCAL_TABLES.contains(&table.as_str()) {
                continue;
            }
            let entity = ENTITIES
                .iter()
                .find(|entity| entity.table == table)
                .unwrap_or_else(|| {
                    panic!("{table} is neither in ENTITIES nor in LOCAL_TABLES: decide which")
                });
            let local = LOCAL_COLUMNS
                .iter()
                .find(|(name, _)| name == table)
                .unwrap_or_else(|| panic!("{table} is missing from LOCAL_COLUMNS"))
                .1;
            let columns: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            for column in &columns {
                let travels = entity.fields.iter().any(|field| field.column == column)
                    || matches!(entity.key, Key::Column(key) if key == column);
                let stays = local.contains(&column.as_str());
                assert!(
                    travels ^ stays,
                    "{table}.{column} must be either in ENTITIES or in LOCAL_COLUMNS, \
                     and it is in {}",
                    if travels { "both" } else { "neither" }
                );
            }
            for field in entity.fields {
                assert!(
                    columns.contains(&field.column.to_string()),
                    "{table} has no column {}",
                    field.column
                );
            }
        }
        for table in LOCAL_TABLES {
            assert!(tables.contains(&table.to_string()), "no table {table}");
        }
    }

    /// The events' field names outlive every column they were named after.
    #[test]
    fn test_field_names_are_unique_within_an_entity() {
        for entity in ENTITIES {
            let names: BTreeSet<_> = entity.fields.iter().map(|field| field.name).collect();
            assert_eq!(names.len(), entity.fields.len(), "{}", entity.kind);
            assert!(!names.contains("present"), "present is spoken for");
        }
    }

    #[test]
    fn test_the_field_allowlists_cover_every_real_value() {
        for status in crate::db::Status::ALL {
            assert!(STATUSES.contains(&status.as_str()));
        }
        for source in ["idgames", "doomwiki", "doomworld", "url", "local"] {
            assert!(source.parse::<SourceType>().is_ok());
            assert!(SOURCE_TYPES.contains(&source));
        }
    }

    #[test]
    fn test_a_new_wad_is_logged_whole() {
        let conn = setup();
        let wad = db::add_wad(
            &conn,
            &NewWad::new("Sunlust", SourceType::Idgames)
                .source_id("18000")
                .author("Ribbiks"),
        )
        .unwrap();

        let events = outbox(&conn);
        assert_eq!(events.len(), 1);
        let (op, kind, key, fields) = &events[0];
        assert_eq!((op.as_str(), kind.as_str()), ("add", "wad"));
        assert_eq!(key, "idgames-18000");
        assert_eq!(key, &uid_of(&conn, wad));
        assert_eq!(fields["title"], "Sunlust");
        assert_eq!(fields["author"], "Ribbiks");
        assert_eq!(fields["data_key"], "idgames-18000");
    }

    #[test]
    fn test_what_a_source_names_it_is_its_uid_and_nothing_else_is() {
        let conn = setup();
        let named = |source: SourceType, id: &str| {
            let wad = db::add_wad(&conn, &NewWad::new("x", source).source_id(id)).unwrap();
            uid_of(&conn, wad)
        };
        assert_eq!(named(SourceType::Idgames, "1"), "idgames-1");
        assert_eq!(named(SourceType::Doomwiki, "2"), "doomwiki-2");
        assert_eq!(named(SourceType::Doomworld, "3"), "doomworld-3");

        // Not one that names things, or not a number, which would go in a folder name.
        for uid in [
            named(SourceType::Url, "4"),
            named(SourceType::Idgames, "../../etc"),
            named(SourceType::Idgames, ""),
        ] {
            assert_eq!(uid.len(), 32, "{uid}");
            assert!(uid.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    /// `force` gets a second row past the duplicate check, and it is its own row.
    #[test]
    fn test_a_forced_second_import_gets_a_uid_of_its_own() {
        let conn = setup();
        let new = NewWad::new("x", SourceType::Idgames).source_id("1");
        let first = db::add_wad(&conn, &new).unwrap();
        let second = db::add_wad(&conn, &new).unwrap();
        assert_eq!(uid_of(&conn, first), "idgames-1");
        assert_eq!(uid_of(&conn, second).len(), 32);
    }

    /// A `delete` is final for a uid, so coming back has to mean another one.
    #[test]
    fn test_importing_again_after_a_purge_makes_a_new_identity() {
        let conn = setup();
        let new = NewWad::new("x", SourceType::Idgames).source_id("1");
        let first = db::add_wad(&conn, &new).unwrap();
        db::delete_wad(&conn, first, true).unwrap();
        let again = db::add_wad(&conn, &new).unwrap();
        assert_ne!(uid_of(&conn, again), "idgames-1");
    }

    /// A trigger inside the statement must not disturb what the caller gets back.
    #[test]
    fn test_rowids_and_change_counts_are_what_they_were() {
        let conn = setup();
        conn.execute(
            "INSERT INTO sync_outbox (op, kind, key) VALUES ('set', 'x', 'y')",
            [],
        )
        .unwrap();
        let wad = db::add_wad(&conn, &NewWad::new("a", SourceType::Local)).unwrap();
        assert_eq!(conn.last_insert_rowid(), wad);
        let changed = conn
            .execute("UPDATE wads SET title = 'b' WHERE id = ?1", [wad])
            .unwrap();
        assert_eq!(changed, 1);
    }

    #[test]
    fn test_only_what_changed_is_logged() {
        let conn = setup();
        let wad = add_wad(&conn);
        clear(&conn);

        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new()
                .set_text("notes", Some("good".into()))
                .set_int("rating", Some(5))
                .set_text("title", Some("Test".into())),
        )
        .unwrap();

        let events = outbox(&conn);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].3, json!({ "notes": "good", "rating": 5 }));
    }

    #[test]
    fn test_clearing_a_field_is_logged_as_clearing_it() {
        let conn = setup();
        let wad = add_wad(&conn);
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("notes", Some("x".into())),
        )
        .unwrap();
        clear(&conn);

        db::update_wad(&conn, wad, &WadUpdate::new().set_text("notes", None)).unwrap();
        assert_eq!(outbox(&conn)[0].3, json!({ "notes": null }));
    }

    /// Where a file is, and when it was hashed, are true of one machine.
    #[test]
    fn test_nothing_about_where_a_file_is_leaves_the_machine() {
        let conn = setup();
        let wad = add_wad(&conn);
        clear(&conn);

        conn.execute(
            "UPDATE wads SET cached_path = '/a', last_known_path = '/b', file_mtime = 5, \
             updated_at = 'now' WHERE id = ?1",
            [wad],
        )
        .unwrap();
        assert!(outbox(&conn).is_empty());
    }

    #[test]
    fn test_a_path_stays_and_a_name_travels() {
        let conn = setup();
        let wad = add_wad(&conn);
        clear(&conn);
        let set = |field, value: &str| {
            db::update_wad(
                &conn,
                wad,
                &WadUpdate::new().set_text(field, Some(value.into())),
            )
            .unwrap();
        };

        set("custom_iwad", "doom2");
        set("custom_iwad", "/home/me/iwads/doom2.wad");
        set("custom_sourceport", "uzdoom");
        set("custom_sourceport", "C:\\Games\\uzdoom.exe");

        let said: Vec<Value> = outbox(&conn).into_iter().map(|event| event.3).collect();
        assert_eq!(
            said,
            vec![
                json!({ "custom_iwad": "doom2" }),
                json!({ "custom_sourceport": "uzdoom" })
            ]
        );
    }

    /// In a whole-row event too, where the path would go along with everything else.
    #[test]
    fn test_a_path_does_not_travel_with_a_new_row_either() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (title, source_type, custom_iwad, custom_sourceport) \
             VALUES ('x', 'local', '/iwads/doom2.wad', 'uzdoom')",
            [],
        )
        .unwrap();
        let fields = &outbox(&conn)[0].3;
        assert_eq!(fields["custom_iwad"], Value::Null);
        assert_eq!(fields["custom_sourceport"], "uzdoom");
    }

    #[test]
    fn test_the_columns_that_say_what_a_file_is_travel_together() {
        let conn = setup();
        let wad = add_wad(&conn);
        conn.execute(
            "UPDATE wads SET file_size = 4, file_sha256 = 'a', file_sha1 = 'b', \
             content_sha256 = 'c' WHERE id = ?1",
            [wad],
        )
        .unwrap();
        clear(&conn);

        // The same size and a different file: nothing should be left to imply the old one.
        conn.execute("UPDATE wads SET file_sha256 = 'z' WHERE id = ?1", [wad])
            .unwrap();
        assert_eq!(
            outbox(&conn)[0].3,
            json!({ "file_size": 4, "file_sha256": "z", "file_sha1": "b", "content_sha256": "c" })
        );
    }

    #[test]
    fn test_a_session_and_what_comes_of_it_are_logged_with_global_names() {
        let conn = setup();
        let wad = add_wad(&conn);
        let uid = uid_of(&conn, wad);
        clear(&conn);

        let session = db::start_session(&conn, wad, Some("dsda-doom"), Some("abc")).unwrap();
        let playthrough = db::start_playthrough(&conn, wad).unwrap();
        conn.execute(
            "UPDATE sessions SET playthrough_id = ?1 WHERE id = ?2",
            [playthrough, session],
        )
        .unwrap();

        let events = outbox(&conn);
        assert_eq!(events[0].0, "add");
        assert_eq!(events[0].1, "session");
        assert_eq!(events[0].3["wad"], Value::String(uid.clone()));
        assert_eq!(events[0].3["sourceport"], "dsda-doom");

        let playthrough_uid: String = conn
            .query_row(
                "SELECT uid FROM playthroughs WHERE id = ?1",
                [playthrough],
                |row| row.get(0),
            )
            .unwrap();
        let link = events
            .iter()
            .find(|event| event.1 == "session" && event.0 == "set")
            .unwrap();
        assert_eq!(link.3, json!({ "playthrough": playthrough_uid }));
    }

    #[test]
    fn test_every_row_gets_a_uid_however_it_was_inserted() {
        let conn = setup();
        let wad = add_wad(&conn);
        for sql in [
            "INSERT INTO sessions (wad_id, started_at) VALUES (?1, 'now')",
            "INSERT INTO wad_completions (wad_id) VALUES (?1)",
            "INSERT INTO playthroughs (wad_id) VALUES (?1)",
        ] {
            conn.execute(sql, [wad]).unwrap();
        }
        for table in ["wads", "sessions", "wad_completions", "playthroughs"] {
            let missing: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE uid IS NULL"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(missing, 0, "{table}");
        }
    }

    #[test]
    fn test_a_tag_going_and_coming_back_is_logged_each_time() {
        let conn = setup();
        let wad = add_wad(&conn);
        let uid = uid_of(&conn, wad);
        clear(&conn);

        db::add_tag(&conn, wad, "Slaughter").unwrap();
        db::add_tag(&conn, wad, "slaughter").unwrap(); // already there: nothing happened
        db::remove_tag(&conn, wad, "slaughter").unwrap();

        let events = outbox(&conn);
        assert_eq!(events.len(), 2);
        let key = format!("{uid}/slaughter");
        assert_eq!(events[0].2, key);
        assert_eq!(
            events[0].3,
            json!({ "wad": uid, "tag": "slaughter", "present": 1 })
        );
        assert_eq!(events[1].2, key);
        assert_eq!(
            events[1].3,
            json!({ "wad": uid, "tag": "slaughter", "present": 0 })
        );
    }

    /// The WAD's own `delete` says everything. Everything else going with it is not
    /// news, and would be wrong: a `delete` for a child is what deleting that alone says.
    #[test]
    fn test_a_purge_is_one_event_however_much_goes_with_it() {
        let conn = setup();
        let wad = add_wad(&conn);
        let uid = uid_of(&conn, wad);
        db::add_tag(&conn, wad, "x").unwrap();
        db::start_session(&conn, wad, None, None).unwrap();
        db::add_wad_completion(&conn, wad, None, None, None).unwrap();
        db::start_playthrough(&conn, wad).unwrap();
        clear(&conn);

        db::delete_wad(&conn, wad, true).unwrap();

        let events = outbox(&conn);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].0, "delete");
        assert_eq!(events[0].2, uid);
    }

    /// Whatever else is still on its way about these from another machine has nowhere
    /// to go, and should know that rather than wait for ever.
    #[test]
    fn test_what_goes_with_a_purge_is_remembered_as_gone() {
        let conn = setup();
        let wad = add_wad(&conn);
        let session = db::start_session(&conn, wad, None, None).unwrap();
        let session_uid: String = conn
            .query_row("SELECT uid FROM sessions WHERE id = ?1", [session], |row| {
                row.get(0)
            })
            .unwrap();

        db::delete_wad(&conn, wad, true).unwrap();

        let tombstones: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_tombstones WHERE kind = 'session' AND key = ?1",
                [session_uid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tombstones, 1);
    }

    #[test]
    fn test_deleting_one_completion_is_an_event() {
        let conn = setup();
        let wad = add_wad(&conn);
        let completion = db::add_wad_completion(&conn, wad, None, None, None).unwrap();
        let uid: String = conn
            .query_row(
                "SELECT uid FROM wad_completions WHERE id = ?1",
                [completion],
                |row| row.get(0),
            )
            .unwrap();
        clear(&conn);

        db::delete_wad_completion(&conn, completion).unwrap();
        let events = outbox(&conn);
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0].0.as_str(), events[0].1.as_str(), &events[0].2),
            ("delete", "completion", &uid)
        );
    }

    /// A link is the user's, or came from their library. The facts come from the dataset
    /// on every machine, and are nobody's to send.
    #[test]
    fn test_only_what_the_user_said_about_a_cacoward_is_logged() {
        let conn = setup();
        let wad = add_wad(&conn);
        let uid = uid_of(&conn, wad);
        conn.execute(
            "INSERT INTO cacowards (key, year, category, page_order, wad_title) \
             VALUES ('2025-winner-x', 2025, 'winner', 1, 'X')",
            [],
        )
        .unwrap();
        clear(&conn);
        assert!(
            outbox(&conn).is_empty(),
            "a new row from the dataset is not news"
        );

        conn.execute(
            "UPDATE cacowards SET wad_title = 'Y', year = 2024 WHERE key = '2025-winner-x'",
            [],
        )
        .unwrap();
        assert!(outbox(&conn).is_empty(), "nor is a correction to it");

        conn.execute(
            "UPDATE cacowards SET wad_id = ?1, manual_override = 1 WHERE key = '2025-winner-x'",
            [wad],
        )
        .unwrap();
        let events = outbox(&conn);
        assert_eq!(events[0].2, "2025-winner-x");
        assert_eq!(events[0].3, json!({ "wad": uid, "manual_override": 1 }));
    }

    #[test]
    fn test_a_companion_and_its_link_are_logged_by_hash() {
        let conn = setup();
        let wad = add_wad(&conn);
        let uid = uid_of(&conn, wad);
        clear(&conn);

        let md5 = "0123456789abcdef0123456789abcdef";
        let companion = db::add_companion(&conn, md5, "patch.deh", "/a/b/patch.deh", 9).unwrap();
        db::link_companion_to_wad(&conn, wad, companion).unwrap();
        db::set_companion_enabled(&conn, wad, companion, false).unwrap();
        db::unlink_companion_from_wad(&conn, wad, companion).unwrap();

        let events = outbox(&conn);
        assert_eq!(events[0].1, "companion");
        assert_eq!(events[0].2, md5);
        assert_eq!(
            events[0].3,
            json!({ "filename": "patch.deh", "size": 9, "present": 1 }),
            "the path is this machine's"
        );
        let key = format!("{uid}/{md5}");
        assert_eq!(events[1].2, key);
        assert_eq!(events[1].3["present"], 1);
        assert_eq!(events[2].2, key);
        assert_eq!(
            events[2].3,
            json!({ "wad": uid, "companion": md5, "enabled": 0 })
        );
        assert_eq!(
            events[3].3,
            json!({ "wad": uid, "companion": md5, "present": 0 })
        );
    }

    #[test]
    fn test_a_collection_is_logged_by_name() {
        let conn = setup();
        db::create_collection(&conn, "todo", "status:u", None, true).unwrap();
        db::update_collection(&conn, "todo", Some("status:p"), None, None).unwrap();
        db::delete_collection(&conn, "todo").unwrap();

        let events = outbox(&conn);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.2 == "todo"));
        assert_eq!(events[0].3["query"], "status:u");
        assert_eq!(events[0].3["present"], 1);
        assert_eq!(events[1].3, json!({ "query": "status:p" }));
        assert_eq!(events[2].3, json!({ "present": 0 }));
    }

    /// Other machines' events being applied here are not this machine's to send again.
    #[test]
    fn test_nothing_is_logged_while_events_are_being_applied() {
        let conn = setup();
        let wad = add_wad(&conn);
        clear(&conn);

        conn.execute("INSERT INTO sync_applying VALUES (1)", [])
            .unwrap();
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("notes", Some("x".into())),
        )
        .unwrap();
        db::add_tag(&conn, wad, "x").unwrap();
        db::remove_tag(&conn, wad, "x").unwrap();
        db::start_session(&conn, wad, None, None).unwrap();
        db::add_wad(&conn, &NewWad::new("y", SourceType::Local)).unwrap();
        conn.execute("DELETE FROM sync_applying", []).unwrap();

        assert!(outbox(&conn).is_empty());
    }

    /// Asked of an applying connection, a new row still needs a uid to be a row at all.
    #[test]
    fn test_a_uid_given_is_a_uid_kept() {
        let conn = setup();
        conn.execute(
            "INSERT INTO wads (title, source_type, uid, data_key) \
             VALUES ('x', 'local', 'given', 'kept')",
            [],
        )
        .unwrap();
        let (uid, key): (String, String) = conn
            .query_row("SELECT uid, data_key FROM wads", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((uid.as_str(), key.as_str()), ("given", "kept"));
    }

    /// Reads and goes away on every launch but the one after an upgrade, because this
    /// runs before the window opens and a rebuild takes the write lock.
    #[test]
    fn test_triggers_are_only_rebuilt_when_they_differ() {
        let conn = setup();
        let mut wanted = triggers();
        wanted.sort();
        // What SQLite keeps is exactly what it was given, which is what makes this a read.
        assert_eq!(installed_triggers(&conn).unwrap(), wanted);

        // A build that has changed one, and a library that has lost one, both get them all.
        conn.execute_batch("DROP TRIGGER sync_tags_insert").unwrap();
        conn.execute_batch(
            "DROP TRIGGER sync_tags_delete;
             CREATE TRIGGER sync_tags_delete AFTER DELETE ON tags BEGIN SELECT 1; END",
        )
        .unwrap();
        install(&conn).unwrap();
        assert_eq!(installed_triggers(&conn).unwrap(), wanted);
    }

    #[test]
    fn test_clearing_a_snapshot_takes_what_others_said_with_it() {
        let conn = setup();
        let wad = add_wad(&conn);
        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("stats_snapshot", Some("{}".into())),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO wad_stats_by_machine (wad_id, machine, clock, stats) \
             VALUES (?1, 'other', 'x', '{}')",
            [wad],
        )
        .unwrap();

        db::update_wad(
            &conn,
            wad,
            &WadUpdate::new().set_text("stats_snapshot", None),
        )
        .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM wad_stats_by_machine", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }
}
