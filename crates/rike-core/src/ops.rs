//! Structural edits on a page tree. Pure functions: they change the model and
//! report whether something happened; saving is the caller's job.
//!
//! Top-level blocks are edited as one sibling list. Non-block items (page
//! properties, raw text) keep their position relative to the blocks around
//! them as far as the new block count allows.

use crate::model::{Block, BlockId, Item, Page};
use crate::task::{Marker, parse_marker, with_marker};

/// A block as shown in the flat editing list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleBlock {
    pub id: BlockId,
    pub depth: usize,
}

/// Blocks in document order, skipping the children of collapsed blocks.
pub fn visible_blocks(page: &Page) -> Vec<VisibleBlock> {
    fn go(b: &Block, depth: usize, out: &mut Vec<VisibleBlock>) {
        out.push(VisibleBlock { id: b.id, depth });
        if !b.is_collapsed() {
            for c in &b.children {
                go(c, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for b in page.blocks() {
        go(b, 0, &mut out);
    }
    out
}

pub fn block_mut(page: &mut Page, id: BlockId) -> Option<&mut Block> {
    fn go(blocks: &mut [Block], id: BlockId) -> Option<&mut Block> {
        for b in blocks {
            if b.id == id {
                return Some(b);
            }
            if let Some(found) = go(&mut b.children, id) {
                return Some(found);
            }
        }
        None
    }
    for item in &mut page.items {
        if let Item::Block(b) = item {
            if b.id == id {
                return Some(b);
            }
            if let Some(found) = go(&mut b.children, id) {
                return Some(found);
            }
        }
    }
    None
}

pub fn set_text(page: &mut Page, id: BlockId, text: String) -> bool {
    match block_mut(page, id) {
        Some(b) if b.text != text => {
            b.text = text;
            true
        }
        _ => false,
    }
}

pub fn set_collapsed(page: &mut Page, id: BlockId, collapsed: bool) -> bool {
    match block_mut(page, id) {
        Some(b) if b.is_collapsed() != collapsed => {
            b.set_collapsed(collapsed);
            true
        }
        _ => false,
    }
}

/// Splits the block at byte offset `at` (a char boundary of its text). The
/// head keeps the id and properties. The tail becomes the first child when the
/// block has visible children, otherwise the next sibling. Returns the new id.
pub fn split_block(page: &mut Page, id: BlockId, at: usize) -> Option<BlockId> {
    with_blocks(page, |root| {
        let path = find_path(root, id)?;
        let block = block_at(root, &path);
        if at > block.text.len() || !block.text.is_char_boundary(at) {
            return None;
        }
        let tail = Block::new(block.text.split_off(at));
        let new_id = tail.id;
        if !block.children.is_empty() && !block.is_collapsed() {
            block.children.insert(0, tail);
        } else {
            let (index, parent) = path.split_last()?;
            siblings_mut(root, parent).insert(index + 1, tail);
        }
        Some(new_id)
    })
}

/// Makes the block the last child of its previous sibling.
pub fn indent(page: &mut Page, id: BlockId) -> bool {
    with_blocks(page, |root| {
        let Some(path) = find_path(root, id) else {
            return false;
        };
        let (&index, parent) = path.split_last().expect("path is never empty");
        if index == 0 {
            return false;
        }
        let siblings = siblings_mut(root, parent);
        let block = siblings.remove(index);
        siblings[index - 1].children.push(block);
        true
    })
}

/// Moves the block after its parent. Following siblings stay with the parent.
pub fn outdent(page: &mut Page, id: BlockId) -> bool {
    with_blocks(page, |root| {
        let Some(path) = find_path(root, id) else {
            return false;
        };
        if path.len() < 2 {
            return false;
        }
        let (&index, parent) = path.split_last().expect("path is never empty");
        let block = siblings_mut(root, parent).remove(index);
        let (&parent_index, grandparent) = parent.split_last().expect("depth checked above");
        siblings_mut(root, grandparent).insert(parent_index + 1, block);
        true
    })
}

/// Swaps the block with its previous sibling.
pub fn move_up(page: &mut Page, id: BlockId) -> bool {
    move_by(page, id, -1)
}

/// Swaps the block with its next sibling.
pub fn move_down(page: &mut Page, id: BlockId) -> bool {
    move_by(page, id, 1)
}

fn move_by(page: &mut Page, id: BlockId, delta: isize) -> bool {
    with_blocks(page, |root| {
        let Some(path) = find_path(root, id) else {
            return false;
        };
        let (&index, parent) = path.split_last().expect("path is never empty");
        let siblings = siblings_mut(root, parent);
        let Some(target) = index.checked_add_signed(delta) else {
            return false;
        };
        if target >= siblings.len() {
            return false;
        }
        siblings.swap(index, target);
        true
    })
}

/// Removes a block that has no children. Returns the block to focus next: the
/// previous visible block, or the following one when there is none.
pub fn delete_block(page: &mut Page, id: BlockId) -> Option<BlockId> {
    let visible = visible_blocks(page);
    let pos = visible.iter().position(|v| v.id == id)?;
    let focus = pos
        .checked_sub(1)
        .map(|p| visible[p].id)
        .or_else(|| visible.get(pos + 1).map(|v| v.id))?;
    with_blocks(page, |root| {
        let path = find_path(root, id)?;
        if !block_at(root, &path).children.is_empty() {
            return None;
        }
        let (&index, parent) = path.split_last()?;
        siblings_mut(root, parent).remove(index);
        Some(focus)
    })
}

/// Open task → DONE → no marker; CANCELED → no marker; no marker → TODO.
pub fn toggle_todo(page: &mut Page, id: BlockId) -> bool {
    let Some(block) = block_mut(page, id) else {
        return false;
    };
    let next = match parse_marker(&block.text) {
        Some((m, _)) if m.is_open() => Some(Marker::Done),
        Some(_) => None,
        None => Some(Marker::Todo),
    };
    block.text = with_marker(&block.text, next);
    true
}

/// What a checkbox does: an open task becomes DONE, DONE goes back to TODO.
/// Blocks without a marker and canceled tasks are left alone.
pub fn set_task_done(page: &mut Page, id: BlockId, done: bool) -> bool {
    let Some(block) = block_mut(page, id) else {
        return false;
    };
    let next = match parse_marker(&block.text) {
        Some((m, _)) if m.is_open() && done => Marker::Done,
        Some((Marker::Done, _)) if !done => Marker::Todo,
        _ => return false,
    };
    block.text = with_marker(&block.text, Some(next));
    true
}

/// Runs `f` on all top-level blocks as one list, then puts the other items
/// back next to the blocks they followed.
fn with_blocks<R>(page: &mut Page, f: impl FnOnce(&mut Vec<Block>) -> R) -> R {
    let mut blocks = Vec::new();
    let mut others: Vec<(usize, Item)> = Vec::new();
    for item in std::mem::take(&mut page.items) {
        match item {
            Item::Block(b) => blocks.push(b),
            other => others.push((blocks.len(), other)),
        }
    }
    let result = f(&mut blocks);

    let mut items = Vec::new();
    let mut others = others.into_iter().peekable();
    for (placed, block) in blocks.into_iter().enumerate() {
        while others.peek().is_some_and(|(anchor, _)| *anchor <= placed) {
            items.push(others.next().expect("peeked").1);
        }
        items.push(Item::Block(block));
    }
    items.extend(others.map(|(_, item)| item));
    page.items = items;
    result
}

fn find_path(blocks: &[Block], id: BlockId) -> Option<Vec<usize>> {
    for (i, b) in blocks.iter().enumerate() {
        if b.id == id {
            return Some(vec![i]);
        }
        if let Some(mut rest) = find_path(&b.children, id) {
            rest.insert(0, i);
            return Some(rest);
        }
    }
    None
}

fn siblings_mut<'a>(root: &'a mut Vec<Block>, parent: &[usize]) -> &'a mut Vec<Block> {
    let mut list = root;
    for &i in parent {
        list = &mut list[i].children;
    }
    list
}

fn block_at<'a>(root: &'a mut Vec<Block>, path: &[usize]) -> &'a mut Block {
    let (&last, parent) = path.split_last().expect("path is never empty");
    &mut siblings_mut(root, parent)[last]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_page;
    use crate::serialize::serialize_page;

    fn page(src: &str) -> Page {
        parse_page(src).page
    }

    fn id_of(page: &Page, text: &str) -> BlockId {
        page.walk()
            .into_iter()
            .find(|e| e.block.text == text)
            .unwrap_or_else(|| panic!("no block {text:?}"))
            .block
            .id
    }

    fn out(page: &Page) -> String {
        serialize_page(page)
    }

    #[test]
    fn split_in_the_middle_makes_a_next_sibling() {
        let mut p = page("- hello world\n- other\n");
        let id = id_of(&p, "hello world");
        let new = split_block(&mut p, id, 5).unwrap();
        assert_eq!(out(&p), "- hello\n-  world\n- other\n");
        assert_eq!(id_of(&p, " world"), new);
    }

    #[test]
    fn split_with_visible_children_inserts_first_child() {
        let mut p = page("- a\n\t- b\n");
        let id = id_of(&p, "a");
        split_block(&mut p, id, 1).unwrap();
        assert_eq!(out(&p), "- a\n\t-\n\t- b\n");
    }

    #[test]
    fn split_collapsed_block_keeps_children_with_head() {
        let mut p = page("- a\n  collapsed:: true\n\t- b\n");
        let id = id_of(&p, "a");
        split_block(&mut p, id, 1).unwrap();
        assert_eq!(out(&p), "- a\n  collapsed:: true\n\t- b\n-\n");
    }

    #[test]
    fn split_respects_char_boundaries_and_range() {
        let mut p = page("- 三省\n");
        let id = id_of(&p, "三省");
        assert!(split_block(&mut p, id, 1).is_none());
        assert!(split_block(&mut p, id, 99).is_none());
        assert!(split_block(&mut p, id, 3).is_some());
        assert_eq!(out(&p), "- 三\n- 省\n");
    }

    #[test]
    fn indent_and_outdent() {
        let mut p = page("- a\n- b\n- c\n");
        let b = id_of(&p, "b");
        assert!(indent(&mut p, b));
        assert_eq!(out(&p), "- a\n\t- b\n- c\n");
        assert!(outdent(&mut p, b));
        assert_eq!(out(&p), "- a\n- b\n- c\n");
    }

    #[test]
    fn indent_first_sibling_and_outdent_top_level_are_refused() {
        let mut p = page("- a\n- b\n");
        let a = id_of(&p, "a");
        assert!(!indent(&mut p, a));
        assert!(!outdent(&mut p, a));
        assert_eq!(out(&p), "- a\n- b\n");
    }

    #[test]
    fn outdent_leaves_following_siblings_with_the_parent() {
        let mut p = page("- a\n\t- b\n\t- c\n");
        let b = id_of(&p, "b");
        assert!(outdent(&mut p, b));
        assert_eq!(out(&p), "- a\n\t- c\n- b\n");
    }

    #[test]
    fn indent_carries_children_along() {
        let mut p = page("- a\n- b\n\t- b1\n");
        let b = id_of(&p, "b");
        assert!(indent(&mut p, b));
        assert_eq!(out(&p), "- a\n\t- b\n\t\t- b1\n");
    }

    #[test]
    fn move_up_down_swap_siblings_only() {
        let mut p = page("- a\n- b\n- c\n");
        let b = id_of(&p, "b");
        assert!(move_up(&mut p, b));
        assert_eq!(out(&p), "- b\n- a\n- c\n");
        assert!(!move_up(&mut p, b));
        assert!(move_down(&mut p, b));
        assert!(move_down(&mut p, b));
        assert!(!move_down(&mut p, b));
        assert_eq!(out(&p), "- a\n- c\n- b\n");
    }

    #[test]
    fn delete_returns_previous_visible_block() {
        let mut p = page("- a\n\t- a1\n- b\n");
        let b = id_of(&p, "b");
        let a1 = id_of(&p, "a1");
        assert_eq!(delete_block(&mut p, b), Some(a1));
        assert_eq!(out(&p), "- a\n\t- a1\n");
    }

    #[test]
    fn delete_first_block_focuses_the_next_one() {
        let mut p = page("- a\n- b\n");
        let a = id_of(&p, "a");
        let b = id_of(&p, "b");
        assert_eq!(delete_block(&mut p, a), Some(b));
    }

    #[test]
    fn delete_refuses_blocks_with_children_and_the_last_block() {
        let mut p = page("- a\n\t- b\n");
        let a = id_of(&p, "a");
        assert_eq!(delete_block(&mut p, a), None);
        let mut single = page("- only\n");
        let id = id_of(&single, "only");
        assert_eq!(delete_block(&mut single, id), None);
        assert_eq!(out(&single), "- only\n");
    }

    #[test]
    fn todo_cycle() {
        let mut p = page("- task\n");
        let id = id_of(&p, "task");
        toggle_todo(&mut p, id);
        assert_eq!(out(&p), "- TODO task\n");
        toggle_todo(&mut p, id);
        assert_eq!(out(&p), "- DONE task\n");
        toggle_todo(&mut p, id);
        assert_eq!(out(&p), "- task\n");
        set_text(&mut p, id, "DOING task".into());
        toggle_todo(&mut p, id);
        assert_eq!(out(&p), "- DONE task\n");
        set_text(&mut p, id, "LATER task".into());
        toggle_todo(&mut p, id);
        assert_eq!(out(&p), "- DONE task\n");
    }

    #[test]
    fn checkbox_completes_and_reopens() {
        let mut p = page("- WAIT task\n- plain\n- CANCELED no\n");
        let task = id_of(&p, "WAIT task");
        let plain = id_of(&p, "plain");
        let no = id_of(&p, "CANCELED no");
        assert!(set_task_done(&mut p, task, true));
        assert_eq!(out(&p), "- DONE task\n- plain\n- CANCELED no\n");
        assert!(set_task_done(&mut p, task, false));
        assert!(out(&p).starts_with("- TODO task\n"));
        assert!(!set_task_done(&mut p, plain, true));
        assert!(!set_task_done(&mut p, no, true));
        assert!(!set_task_done(&mut p, no, false));
    }

    #[test]
    fn visible_blocks_skip_collapsed_children() {
        let p = page("- a\n  collapsed:: true\n\t- hidden\n- b\n");
        let visible: Vec<_> = visible_blocks(&p).iter().map(|v| v.depth).collect();
        assert_eq!(visible, vec![0, 0]);
    }

    #[test]
    fn collapse_toggles_the_property() {
        let mut p = page("- a\n\t- b\n");
        let a = id_of(&p, "a");
        assert!(set_collapsed(&mut p, a, true));
        assert_eq!(out(&p), "- a\n  collapsed:: true\n\t- b\n");
        assert!(!set_collapsed(&mut p, a, true));
        assert!(set_collapsed(&mut p, a, false));
        assert_eq!(out(&p), "- a\n\t- b\n");
    }

    #[test]
    fn page_properties_and_raw_stay_in_place() {
        let mut p = page("title:: T\n- a\n- b\n");
        let b = id_of(&p, "b");
        assert!(move_up(&mut p, b));
        assert_eq!(out(&p), "title:: T\n- b\n- a\n");

        let mut q = page("---\nfm\n---\n- a\n- b\n");
        let a = id_of(&q, "a");
        let b = id_of(&q, "b");
        assert!(indent(&mut q, b));
        assert!(set_text(&mut q, a, "a2".into()));
        assert_eq!(out(&q), "---\nfm\n---\n- a2\n\t- b\n");
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let mut p = page("- a\n");
        let ghost = BlockId::new();
        assert!(!indent(&mut p, ghost));
        assert!(!outdent(&mut p, ghost));
        assert!(!move_up(&mut p, ghost));
        assert!(split_block(&mut p, ghost, 0).is_none());
        assert!(delete_block(&mut p, ghost).is_none());
        assert!(!toggle_todo(&mut p, ghost));
    }
}
