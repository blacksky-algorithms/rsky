use libipld::Cid;
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Debug, Deserialize, Serialize)]
pub struct CidSet {
    pub set: HashSet<String>,
}
impl CidSet {
    pub fn new(arr: Option<Vec<Cid>>) -> CidSet {
        let str_arr: Vec<String> = arr
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|cid| cid.to_string())
            .collect::<Vec<String>>();
        CidSet {
            set: HashSet::from_iter(str_arr),
        }
    }

    pub fn add(mut self, cid: Cid) -> CidSet {
        self.set.insert(cid.to_string());
        self
    }

    pub fn add_set(mut self, to_merge: CidSet) -> CidSet {
        for cid in to_merge.to_list() {
            self = self.add(cid);
        }
        self
    }

    pub fn subtract_set(mut self, to_subtract: CidSet) -> CidSet {
        for cid in to_subtract.to_list() {
            self = self.delete(cid);
        }
        self
    }

    pub fn delete(mut self, cid: Cid) -> CidSet {
        self.set.remove(&cid.to_string());
        self
    }

    pub fn has(&self, cid: Cid) -> bool {
        self.set.contains(&cid.to_string())
    }

    pub fn size(&self) -> usize {
        self.set.len()
    }

    pub fn clear(mut self) -> CidSet {
        self.set.clear();
        self
    }

    pub fn to_list(&self) -> Vec<Cid> {
        self.set
            .clone()
            .into_iter()
            .filter_map(|cid| match Cid::from_str(&cid) {
                Ok(r) => Some(r),
                Err(_) => None,
            })
            .collect::<Vec<Cid>>()
    }
}
