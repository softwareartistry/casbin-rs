#[cfg(feature = "runtime-tokio")]
use std::io::Cursor as ioCursor;

use crate::{
    adapter::{Adapter, Filter},
    model::Model,
    util::parse_csv_line,
    Result,
};

use async_trait::async_trait;
use hashlink::LinkedHashSet;

#[cfg(feature = "runtime-tokio")]
use tokio::io::AsyncBufReadExt;

#[cfg(feature = "runtime-tokio")]
use tokio::io::BufReader as ioBufReader;

#[derive(Debug, Default)]
pub struct WasmAdapter {
    policy: LinkedHashSet<Vec<String>>,
    is_filtered: bool,
}

impl WasmAdapter {
    pub async fn parse_from_str_csv<S: AsRef<str>>(s: S) -> Result<Self> {
        let mut reader = ioBufReader::new(ioCursor::new(s.as_ref().as_bytes()));
        let mut adapter = Self::default();
        let mut policy = LinkedHashSet::new();

        loop {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                // EOF reached
                break;
            }

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(tokens) = parse_csv_line(line) {
                if !tokens.is_empty() {
                    policy.insert(tokens);
                }
            }
        }

        adapter.policy = policy;
        adapter.is_filtered = false;

        Ok(adapter)
    }
}

#[async_trait]
impl Adapter for WasmAdapter {
    async fn load_policy(&mut self, m: &mut dyn Model) -> Result<()> {
        self.is_filtered = false;
        for line in self.policy.iter() {
            let sec = &line[0];
            let rule = line[1..].to_vec().clone();

            if let Some(t1) = m.get_mut_model().get_mut(sec) {
                if let Some(t2) = t1.get_mut(sec) {
                    t2.get_mut_policy().insert(rule);
                }
            }
        }

        Ok(())
    }

    async fn load_filtered_policy<'a>(
        &mut self,
        m: &mut dyn Model,
        f: Filter<'a>,
    ) -> Result<()> {
        for line in self.policy.iter() {
            let sec = &line[0];
            let rule = line[1..].to_vec().clone();
            let mut is_filtered = false;

            if sec == "p" {
                for (i, r) in f.p.iter().enumerate() {
                    if !r.is_empty() && r != &rule[i + 1] {
                        is_filtered = true;
                    }
                }
            }
            if sec == "g" {
                for (i, r) in f.g.iter().enumerate() {
                    if !r.is_empty() && r != &rule[i + 1] {
                        is_filtered = true;
                    }
                }
            }

            if !is_filtered {
                if let Some(ast_map) = m.get_mut_model().get_mut(sec) {
                    if let Some(ast) = ast_map.get_mut(sec) {
                        ast.get_mut_policy().insert(rule);
                    }
                }
            } else {
                self.is_filtered = true;
            }
        }
        Ok(())
    }

    async fn save_policy(&mut self, m: &mut dyn Model) -> Result<()> {
        self.policy.clear();

        // In csv structure, ptype and sec is same, using the same logic

        if let Some(ast_map) = m.get_model().get("p") {
            for (ptype, ast) in ast_map {
                if let Some(sec) = ptype.chars().next().map(|x| x.to_string()) {
                    for policy in ast.get_policy() {
                        let mut rule = policy.clone();
                        // rule.insert(0, ptype.clone());
                        rule.insert(0, sec.clone());
                        self.policy.insert(rule);
                    }
                }
            }
        }

        if let Some(ast_map) = m.get_model().get("g") {
            for (ptype, ast) in ast_map {
                if let Some(sec) = ptype.chars().next().map(|x| x.to_string()) {
                    for policy in ast.get_policy() {
                        let mut rule = policy.clone();
                        // rule.insert(0, ptype.clone());
                        rule.insert(0, sec.clone());
                        self.policy.insert(rule);
                    }
                }
            }
        }

        Ok(())
    }

    async fn clear_policy(&mut self) -> Result<()> {
        self.policy.clear();
        self.is_filtered = false;
        Ok(())
    }

    async fn add_policy(
        &mut self,
        sec: &str,
        _ptype: &str,
        mut rule: Vec<String>,
    ) -> Result<bool> {
        // rule.insert(0, ptype.to_owned());
        rule.insert(0, sec.to_owned());

        Ok(self.policy.insert(rule))
    }

    async fn add_policies(
        &mut self,
        sec: &str,
        _ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> Result<bool> {
        let mut all_added = true;
        let rules: Vec<Vec<String>> = rules
            .into_iter()
            .map(|mut rule| {
                // rule.insert(0, ptype.to_owned());
                rule.insert(0, sec.to_owned());
                rule
            })
            .collect();

        for rule in &rules {
            if self.policy.contains(rule) {
                all_added = false;
                return Ok(all_added);
            }
        }
        self.policy.extend(rules);

        Ok(all_added)
    }

    async fn remove_policies(
        &mut self,
        sec: &str,
        _ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> Result<bool> {
        let mut all_removed = true;
        let rules: Vec<Vec<String>> = rules
            .into_iter()
            .map(|mut rule| {
                // rule.insert(0, ptype.to_owned());
                rule.insert(0, sec.to_owned());
                rule
            })
            .collect();

        for rule in &rules {
            if !self.policy.contains(rule) {
                all_removed = false;
                return Ok(all_removed);
            }
        }
        for rule in &rules {
            self.policy.remove(rule);
        }

        Ok(all_removed)
    }

    async fn remove_policy(
        &mut self,
        sec: &str,
        _ptype: &str,
        mut rule: Vec<String>,
    ) -> Result<bool> {
        // rule.insert(0, ptype.to_owned());
        rule.insert(0, sec.to_owned());

        Ok(self.policy.remove(&rule))
    }

    async fn remove_filtered_policy(
        &mut self,
        sec: &str,
        _ptype: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> Result<bool> {
        if field_values.is_empty() {
            return Ok(false);
        }

        let mut tmp = LinkedHashSet::new();
        let mut res = false;
        for rule in &self.policy {
            if sec == rule[0] {
                let mut matched = true;
                for (i, field_value) in field_values.iter().enumerate() {
                    if !field_value.is_empty()
                        && &rule[field_index + i + 2] != field_value
                    {
                        matched = false;
                        break;
                    }
                }

                if matched {
                    res = true;
                } else {
                    tmp.insert(rule.clone());
                }
            } else {
                tmp.insert(rule.clone());
            }
        }
        self.policy = tmp;

        Ok(res)
    }

    fn is_filtered(&self) -> bool {
        self.is_filtered
    }
}
