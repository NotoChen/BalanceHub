use crate::services::agent_cli::{
    self,
    contracts::{SessionContentSearchRequest, SessionSearchTerm},
};
use std::collections::HashSet;

use super::{MAX_SEARCH_QUERY_CHARS, MAX_SEARCH_TERMS};

pub(crate) struct SessionContentSearchCollector<'a> {
    request: &'a SessionContentSearchRequest,
    matched_terms: HashSet<usize>,
    has_content: bool,
}

impl<'a> SessionContentSearchCollector<'a> {
    pub(crate) fn new(request: &'a SessionContentSearchRequest) -> Self {
        Self {
            request,
            matched_terms: HashSet::new(),
            has_content: false,
        }
    }

    pub(crate) fn observe(&mut self, content: &str) {
        let content = content.trim();
        if content.is_empty() {
            return;
        }
        self.has_content = true;
        let folded = content.to_lowercase();
        for term in &self.request.terms {
            if folded.contains(&term.value) {
                self.matched_terms.insert(term.index);
            }
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.matched_terms.len() == self.request.terms.len()
    }

    pub(crate) fn finish(self) -> agent_cli::contracts::SessionContentSearchResult {
        agent_cli::contracts::SessionContentSearchResult {
            matched_term_indexes: self.matched_terms.into_iter().collect(),
            has_content: self.has_content,
        }
    }
}

pub(crate) fn combine_content_search_results(
    results: impl IntoIterator<Item = agent_cli::contracts::SessionContentSearchResult>,
) -> agent_cli::contracts::SessionContentSearchResult {
    let mut combined = agent_cli::contracts::SessionContentSearchResult::default();
    for result in results {
        for index in result.matched_term_indexes {
            if !combined.matched_term_indexes.contains(&index) {
                combined.matched_term_indexes.push(index);
            }
        }
        combined.has_content |= result.has_content;
    }
    combined
}

pub(crate) fn truncate_text(value: &str, limit: usize) -> (String, bool) {
    if limit == 0 {
        return (String::new(), !value.is_empty());
    }
    let mut chars = value.chars();
    let mut text = chars.by_ref().take(limit).collect::<String>();
    let truncated = chars.next().is_some();
    if truncated && limit > 3 {
        text.push_str("...");
    }
    (text, truncated)
}

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

    pub(crate) fn content_request(&self) -> SessionContentSearchRequest {
        SessionContentSearchRequest {
            terms: self
                .terms
                .iter()
                .enumerate()
                .map(|(index, value)| SessionSearchTerm {
                    index,
                    value: value.clone(),
                })
                .collect(),
        }
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
