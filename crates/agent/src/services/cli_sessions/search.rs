use crate::services::agent_cli::{
    self,
    contracts::{SessionContentSearchRequest, SessionSearchTerm},
};
use std::collections::HashSet;

use super::{MAX_SEARCH_QUERY_CHARS, MAX_SEARCH_TERMS};

pub(crate) struct SearchQuery {
    terms: Vec<String>,
}

impl SearchQuery {
    pub(crate) fn new(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.chars().count() > MAX_SEARCH_QUERY_CHARS {
            return Err(format!(
                "搜索关键字过长，最多支持 {MAX_SEARCH_QUERY_CHARS} 个字符"
            ));
        }
        let mut seen = HashSet::new();
        let terms = value
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|term| !term.is_empty() && seen.insert(term.clone()))
            .take(MAX_SEARCH_TERMS)
            .collect();
        Ok(Self { terms })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

pub(crate) struct SearchAccumulator<'a> {
    query: &'a SearchQuery,
    matched_terms: HashSet<usize>,
}

impl<'a> SearchAccumulator<'a> {
    pub(crate) fn new(query: &'a SearchQuery) -> Self {
        Self {
            query,
            matched_terms: HashSet::new(),
        }
    }

    pub(crate) fn observe(&mut self, value: &str) {
        let folded = value.to_lowercase();
        for (index, term) in self.query.terms.iter().enumerate() {
            if folded.contains(term) {
                self.matched_terms.insert(index);
            }
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.matched_terms.len() == self.query.terms.len()
    }

    pub(crate) fn content_request(&self) -> SessionContentSearchRequest {
        SessionContentSearchRequest {
            terms: self
                .query
                .terms
                .iter()
                .enumerate()
                .filter(|(index, _)| !self.matched_terms.contains(index))
                .map(|(index, value)| SessionSearchTerm {
                    index,
                    value: value.clone(),
                })
                .collect(),
        }
    }

    pub(crate) fn merge_content(
        &mut self,
        result: agent_cli::contracts::SessionContentSearchResult,
    ) {
        self.matched_terms.extend(result.matched_term_indexes);
    }
}
