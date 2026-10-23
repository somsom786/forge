//! Code intelligence for a project: the Code Map and symbol search over its main checkout. Read-only; computed on
//! demand from the in-memory index in `codeintel` (no database state).

use super::Core;
use crate::codeintel::{self, CodeMap, SymbolHit};
use crate::error::Result;
use std::path::Path;

/// Upper bound on search results a caller may ask for.
const MAX_SYMBOL_RESULTS: usize = 500;

impl Core {
    pub fn code_map(&self, project_id: &str) -> Result<CodeMap> {
        let p = self.store.get_project(project_id)?;
        let ix = codeintel::index_for(Path::new(&p.root_path))?;
        Ok(codeintel::code_map(&ix))
    }

    /// Symbols matching `query` (by name), or every symbol of `file` when the query is empty.
    pub fn code_symbols(&self, project_id: &str, query: &str, file: Option<&str>, limit: usize) -> Result<Vec<SymbolHit>> {
        let p = self.store.get_project(project_id)?;
        let ix = codeintel::index_for(Path::new(&p.root_path))?;
        Ok(codeintel::search(&ix, query, file, limit.clamp(1, MAX_SYMBOL_RESULTS)))
    }
}
