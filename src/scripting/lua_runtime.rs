use crate::timing::Note;
use mlua::Lua;

pub struct LuaRuntime {
    pub lua: Lua,
}

impl LuaRuntime {
    pub fn new() -> Result<Self, mlua::Error> {
        let lua = Lua::new();
        Ok(Self { lua })
    }

    pub fn execute(&self, code: &str) -> Result<(), mlua::Error> {
        self.lua.load(code).exec()
    }

    pub fn execute_pattern(&self, code: &str) -> Result<Vec<Note>, mlua::Error> {
        let result: mlua::Table = self.lua.load(code).eval()?;

        let mut notes = Vec::new();
        for pair in result.pairs::<usize, mlua::Table>() {
            let (_, note_table) = pair?;

            let pitch: u8 = note_table.get("pitch")?;
            let velocity: u8 = note_table.get("velocity")?;
            let start_beat: f32 = note_table.get("start_beat")?;
            let duration_beats: f32 = note_table.get("duration_beats")?;

            notes.push(Note {
                pitch,
                velocity,
                start_beat,
                duration_beats,
            });
        }

        Ok(notes)
    }
}
