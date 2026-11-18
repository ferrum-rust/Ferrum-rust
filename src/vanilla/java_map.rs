//! Java 8+ HashMap traversal for comparable Long/String structure keys.
//! Buckets, resize splits, treeification and red/black root-to-front movements
//! affect structure population RNG order. Not a general replacement for HashMap.
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
pub trait JavaKey: Eq {
    fn hash_code(&self) -> i32;
    fn compare(&self, other: &Self) -> Ordering;
}
impl JavaKey for i64 {
    fn hash_code(&self) -> i32 {
        (*self as u64 ^ (*self as u64 >> 32)) as i32
    }
    fn compare(&self, other: &Self) -> Ordering {
        self.cmp(other)
    }
}
impl JavaKey for String {
    fn hash_code(&self) -> i32 {
        self.encode_utf16()
            .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
    }
    fn compare(&self, other: &Self) -> Ordering {
        self.encode_utf16().cmp(other.encode_utf16())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Node<K, V> {
    key: K,
    value: V,
    hash: i32,
    next: Option<usize>,
    prev: Option<usize>,
    parent: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    red: bool,
    tree: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JavaMap<K, V> {
    buckets: Vec<Option<usize>>,
    nodes: Vec<Node<K, V>>,
    threshold: usize,
}
impl<K, V> Default for JavaMap<K, V> {
    fn default() -> Self {
        Self {
            buckets: Vec::new(),
            nodes: Vec::new(),
            threshold: 0,
        }
    }
}
impl<K: JavaKey, V> JavaMap<K, V> {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    fn hash(key: &K) -> i32 {
        let h = key.hash_code() as u32;
        (h ^ (h >> 16)) as i32
    }
    fn find(&self, key: &K) -> Option<usize> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = Self::hash(key);
        let mut at = self.buckets[hash as usize & (self.buckets.len() - 1)];
        while let Some(i) = at {
            if self.nodes[i].hash == hash && self.nodes[i].key == *key {
                return Some(i);
            }
            at = self.nodes[i].next;
        }
        None
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.find(key).map(|i| &self.nodes[i].value)
    }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.find(key).map(|i| &mut self.nodes[i].value)
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.find(key).is_some()
    }
    pub fn indices(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        for bucket in &self.buckets {
            let mut at = *bucket;
            while let Some(i) = at {
                out.push(i);
                at = self.nodes[i].next;
            }
        }
        out
    }
    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> {
        self.indices()
            .into_iter()
            .map(|i| (&self.nodes[i].key, &self.nodes[i].value))
    }
    pub fn entry_mut(&mut self, index: usize) -> (&K, &mut V) {
        let n = &mut self.nodes[index];
        (&n.key, &mut n.value)
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(i) = self.find(&key) {
            return Some(std::mem::replace(&mut self.nodes[i].value, value));
        }
        if self.buckets.is_empty() {
            self.resize();
        }
        let hash = Self::hash(&key);
        let bucket = hash as usize & (self.buckets.len() - 1);
        let index = self.nodes.len();
        self.nodes.push(Node {
            key,
            value,
            hash,
            next: None,
            prev: None,
            parent: None,
            left: None,
            right: None,
            red: false,
            tree: false,
        });
        if let Some(head) = self.buckets[bucket] {
            if self.nodes[head].tree {
                self.insert_tree(bucket, index);
            } else {
                let mut tail = head;
                let mut count = 0;
                while let Some(next) = self.nodes[tail].next {
                    tail = next;
                    count += 1;
                }
                self.nodes[tail].next = Some(index);
                if count >= 7 {
                    if self.buckets.len() < 64 {
                        self.resize();
                    } else {
                        self.treeify(bucket);
                    }
                }
            }
        } else {
            self.buckets[bucket] = Some(index);
        }
        if self.nodes.len() > self.threshold {
            self.resize();
        }
        None
    }
    fn resize(&mut self) {
        let old = self.buckets.len();
        let capacity = if old == 0 { 16 } else { old * 2 };
        let previous = std::mem::replace(&mut self.buckets, vec![None; capacity]);
        self.threshold = capacity * 3 / 4;
        for (bucket, head) in previous.into_iter().enumerate() {
            let Some(head) = head else {
                continue;
            };
            let tree = self.nodes[head].tree;
            let mut groups = [Vec::new(), Vec::new()];
            let mut at = Some(head);
            while let Some(i) = at {
                at = self.nodes[i].next;
                groups[usize::from(self.nodes[i].hash as usize & old != 0)].push(i);
            }
            let split = !groups[0].is_empty() && !groups[1].is_empty();
            for (side, list) in groups.into_iter().enumerate() {
                if list.is_empty() {
                    continue;
                }
                let target = bucket + side * old;
                self.buckets[target] = Some(list[0]);
                for (j, &i) in list.iter().enumerate() {
                    self.nodes[i].next = list.get(j + 1).copied();
                    self.nodes[i].prev = j.checked_sub(1).map(|j| list[j]);
                }
                if tree {
                    if list.len() <= 6 {
                        for i in list {
                            self.nodes[i].tree = false;
                            self.nodes[i].parent = None;
                            self.nodes[i].left = None;
                            self.nodes[i].right = None;
                            self.nodes[i].red = false;
                        }
                    } else if split {
                        self.treeify(target);
                    }
                }
            }
        }
    }
    fn direction(&self, a: usize, b: usize) -> Ordering {
        self.nodes[a]
            .hash
            .cmp(&self.nodes[b].hash)
            .then_with(|| self.nodes[a].key.compare(&self.nodes[b].key))
    }
    fn treeify(&mut self, bucket: usize) {
        let mut root = None;
        let mut at = self.buckets[bucket];
        let mut previous = None;
        while let Some(i) = at {
            at = self.nodes[i].next;
            self.nodes[i].prev = previous;
            previous = Some(i);
            self.nodes[i].tree = true;
            self.nodes[i].left = None;
            self.nodes[i].right = None;
            if let Some(r) = root {
                let mut p = r;
                loop {
                    let left = self.direction(i, p) != Ordering::Greater;
                    let next = if left {
                        self.nodes[p].left
                    } else {
                        self.nodes[p].right
                    };
                    if let Some(next) = next {
                        p = next;
                    } else {
                        self.nodes[i].parent = Some(p);
                        if left {
                            self.nodes[p].left = Some(i);
                        } else {
                            self.nodes[p].right = Some(i);
                        }
                        root = Some(self.balance(r, i));
                        break;
                    }
                }
            } else {
                self.nodes[i].parent = None;
                self.nodes[i].red = false;
                root = Some(i);
            }
        }
        if let Some(root) = root {
            self.front(bucket, root);
        }
    }
    fn insert_tree(&mut self, bucket: usize, i: usize) {
        let mut root = self.buckets[bucket].unwrap();
        while let Some(parent) = self.nodes[root].parent {
            root = parent;
        }
        let mut p = root;
        loop {
            let left = self.direction(i, p) != Ordering::Greater;
            let child = if left {
                self.nodes[p].left
            } else {
                self.nodes[p].right
            };
            if let Some(child) = child {
                p = child;
                continue;
            }
            let next = self.nodes[p].next;
            self.nodes[i].tree = true;
            self.nodes[i].next = next;
            self.nodes[i].parent = Some(p);
            self.nodes[i].prev = Some(p);
            self.nodes[p].next = Some(i);
            if let Some(next) = next {
                self.nodes[next].prev = Some(i);
            }
            if left {
                self.nodes[p].left = Some(i);
            } else {
                self.nodes[p].right = Some(i);
            }
            root = self.balance(root, i);
            self.front(bucket, root);
            break;
        }
    }
    fn front(&mut self, bucket: usize, root: usize) {
        let first = self.buckets[bucket];
        if first == Some(root) {
            return;
        }
        self.buckets[bucket] = Some(root);
        let next = self.nodes[root].next;
        let prev = self.nodes[root].prev;
        if let Some(next) = next {
            self.nodes[next].prev = prev;
        }
        if let Some(prev) = prev {
            self.nodes[prev].next = next;
        }
        if let Some(first) = first {
            self.nodes[first].prev = Some(root);
        }
        self.nodes[root].next = first;
        self.nodes[root].prev = None;
    }
    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        if let Some(r) = self.nodes[p].right {
            let rl = self.nodes[r].left;
            self.nodes[p].right = rl;
            if let Some(rl) = rl {
                self.nodes[rl].parent = Some(p);
            }
            let pp = self.nodes[p].parent;
            self.nodes[r].parent = pp;
            if let Some(pp) = pp {
                if self.nodes[pp].left == Some(p) {
                    self.nodes[pp].left = Some(r);
                } else {
                    self.nodes[pp].right = Some(r);
                }
            } else {
                root = r;
                self.nodes[r].red = false;
            }
            self.nodes[r].left = Some(p);
            self.nodes[p].parent = Some(r);
        }
        root
    }
    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        if let Some(l) = self.nodes[p].left {
            let lr = self.nodes[l].right;
            self.nodes[p].left = lr;
            if let Some(lr) = lr {
                self.nodes[lr].parent = Some(p);
            }
            let pp = self.nodes[p].parent;
            self.nodes[l].parent = pp;
            if let Some(pp) = pp {
                if self.nodes[pp].right == Some(p) {
                    self.nodes[pp].right = Some(l);
                } else {
                    self.nodes[pp].left = Some(l);
                }
            } else {
                root = l;
                self.nodes[l].red = false;
            }
            self.nodes[l].right = Some(p);
            self.nodes[p].parent = Some(l);
        }
        root
    }
    fn balance(&mut self, mut root: usize, mut x: usize) -> usize {
        self.nodes[x].red = true;
        loop {
            let Some(mut p) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return x;
            };
            let Some(mut g) = self.nodes[p].parent else {
                return root;
            };
            if !self.nodes[p].red {
                return root;
            }
            let left = self.nodes[g].left == Some(p);
            let uncle = if left {
                self.nodes[g].right
            } else {
                self.nodes[g].left
            };
            if uncle.is_some_and(|u| self.nodes[u].red) {
                self.nodes[uncle.unwrap()].red = false;
                self.nodes[p].red = false;
                self.nodes[g].red = true;
                x = g;
                continue;
            }
            if if left {
                self.nodes[p].right == Some(x)
            } else {
                self.nodes[p].left == Some(x)
            } {
                x = p;
                root = if left {
                    self.rotate_left(root, x)
                } else {
                    self.rotate_right(root, x)
                };
                p = self.nodes[x].parent.unwrap();
                g = self.nodes[p].parent.unwrap();
            }
            self.nodes[p].red = false;
            self.nodes[g].red = true;
            root = if left {
                self.rotate_right(root, g)
            } else {
                self.rotate_left(root, g)
            };
        }
    }
}
/// ChunkCoordIntPair.chunkXZ2Int: X in the low unsigned32, Z in the high32.
pub fn chunk_key(x: i32, z: i32) -> i64 {
    ((x as u32 as u64) | ((z as u32 as u64) << 32)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::Value;
    fn check<K: JavaKey + Clone + ToString + Serialize + DeserializeOwned>(
        row: &Value,
        convert: impl Fn(&str) -> K,
    ) {
        let mut map = JavaMap::<K, usize>::default();
        for (i, input) in row["input"].as_array().unwrap().iter().enumerate() {
            let key = convert(input.as_str().unwrap());
            map.insert(key.clone(), i);
            assert_eq!(map.get(&key), Some(&i));
            let actual = map
                .entries()
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>();
            assert_eq!(
                serde_json::json!(actual),
                row["orders"][i],
                "{} insertion {i}",
                row["kind"]
            );
            if i % 17 == 0 {
                map = serde_json::from_slice(&serde_json::to_vec(&map).unwrap()).unwrap();
            }
        }
    }
    #[test]
    fn original_java_hashmap_resize_tree_bins_and_replacements_match() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-hashmap.json"))
                .unwrap();
        for row in fixture["cases"].as_array().unwrap() {
            if row["kind"] == "long" {
                check(row, |s| s.parse::<i64>().unwrap());
            } else {
                check(row, str::to_owned);
            }
        }
    }
}
