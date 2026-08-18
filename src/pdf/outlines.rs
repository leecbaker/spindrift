use super::*;

pub(super) fn outline_plan(document: &Document, first_outline_id: usize) -> Option<OutlinePlan> {
    let tree = bookmark_tree(&document.bookmarks);
    if tree.is_empty() {
        return None;
    }

    let root_id = first_outline_id;
    let mut nodes = Vec::new();
    let mut next_id = first_outline_id + 1;
    let visible_count = append_outline_nodes(
        &tree,
        root_id,
        &mut next_id,
        &mut nodes,
        document.pages.len(),
    );
    Some(OutlinePlan {
        root_id,
        nodes,
        visible_count,
    })
}

pub(super) fn bookmark_tree(bookmarks: &[Bookmark]) -> Vec<BookmarkTreeNode> {
    let mut root = Vec::new();
    let mut skipped_levels = Vec::<u32>::new();
    let mut previous_level = 0u32;
    let mut paths = Vec::<Vec<usize>>::new();

    for bookmark in bookmarks {
        let level = bookmark.level;
        if level == 0 {
            continue;
        }

        if level > previous_level {
            skipped_levels.push(level - previous_level - 1);
        } else {
            let mut temp = level;
            while temp < previous_level {
                let Some(skip) = skipped_levels.pop() else {
                    break;
                };
                temp += 1 + skip;
            }
            if temp > previous_level {
                skipped_levels.push(temp - previous_level - 1);
            }
        }

        previous_level = level;
        let depth = level.saturating_sub(skipped_levels.iter().sum::<u32>());
        if depth == 0 {
            continue;
        }
        let depth_index = (depth - 1) as usize;
        paths.truncate(depth_index);

        let node = BookmarkTreeNode {
            bookmark: bookmark.clone(),
            children: Vec::new(),
        };
        if depth_index == 0 {
            root.push(node);
            paths.push(vec![root.len() - 1]);
        } else if let Some(parent_path) = paths.get(depth_index - 1).cloned()
            && let Some(parent_children) = bookmark_children_mut(&mut root, &parent_path)
        {
            parent_children.push(node);
            let mut path = parent_path;
            path.push(parent_children.len() - 1);
            paths.push(path);
        }
    }

    root
}

pub(super) fn bookmark_children_mut<'a>(
    nodes: &'a mut [BookmarkTreeNode],
    path: &[usize],
) -> Option<&'a mut Vec<BookmarkTreeNode>> {
    let (first, rest) = path.split_first()?;
    let mut node = nodes.get_mut(*first)?;
    for index in rest {
        node = node.children.get_mut(*index)?;
    }
    Some(&mut node.children)
}

pub(super) fn append_outline_nodes(
    siblings: &[BookmarkTreeNode],
    parent_id: usize,
    next_id: &mut usize,
    output: &mut Vec<OutlineNodePlan>,
    page_count: usize,
) -> i32 {
    let ids = siblings
        .iter()
        .map(|_| {
            let id = *next_id;
            *next_id += 1;
            id
        })
        .collect::<Vec<_>>();
    let mut visible_count = siblings.len() as i32;
    for (index, node) in siblings.iter().enumerate() {
        let first_child_id = (!node.children.is_empty()).then_some(*next_id);
        let last_child_id =
            first_child_id.map(|first_child_id| first_child_id + node.children.len() - 1);
        let child_count =
            append_outline_nodes(&node.children, ids[index], next_id, output, page_count);
        visible_count += match node.bookmark.state {
            BookmarkState::Open => child_count,
            BookmarkState::Closed => 0,
        };
        output.push(OutlineNodePlan {
            id: ids[index],
            label: node.bookmark.label.clone(),
            page_index: node.bookmark.page_index.min(page_count.saturating_sub(1)),
            target: node.bookmark.target(),
            parent_id,
            prev_id: index.checked_sub(1).map(|prev| ids[prev]),
            next_id: ids.get(index + 1).cloned(),
            first_child_id,
            last_child_id,
            child_count: match node.bookmark.state {
                BookmarkState::Open => child_count,
                BookmarkState::Closed => -child_count,
            },
        });
    }
    visible_count
}
