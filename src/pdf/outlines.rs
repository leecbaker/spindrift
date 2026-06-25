use super::*;

pub(super) fn catalog_dictionary(outline_plan: Option<&OutlinePlan>) -> String {
    let outlines = outline_plan
        .map(|plan| format!(" /Outlines {} 0 R", plan.root_id))
        .unwrap_or_default();
    format!("<< /Type /Catalog /Pages 2 0 R{outlines} >>\n")
}

pub(super) fn outline_plan(document: &Document, first_outline_id: usize) -> Option<OutlinePlan> {
    let tree = bookmark_tree(&document.bookmarks);
    if tree.is_empty() {
        return None;
    }

    let root_id = first_outline_id;
    let mut nodes = Vec::new();
    let mut next_id = first_outline_id + 1;
    let visible_count = append_outline_nodes(&tree, root_id, &mut next_id, &mut nodes);
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
        let child_count = append_outline_nodes(&node.children, ids[index], next_id, output);
        visible_count += match node.bookmark.state {
            BookmarkState::Open => child_count,
            BookmarkState::Closed => 0,
        };
        output.push(OutlineNodePlan {
            id: ids[index],
            bookmark: node.bookmark.clone(),
            parent_id,
            prev_id: index.checked_sub(1).map(|prev| ids[prev]),
            next_id: ids.get(index + 1).copied(),
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

pub(super) fn outline_objects(
    plan: &OutlinePlan,
    first_page_id: usize,
    document: &Document,
) -> Vec<(usize, Vec<u8>)> {
    let mut objects = Vec::new();
    let top_level_ids = plan
        .nodes
        .iter()
        .filter(|node| node.parent_id == plan.root_id)
        .map(|node| node.id)
        .collect::<Vec<_>>();
    let first = top_level_ids.iter().min().copied();
    let last = top_level_ids.iter().max().copied();
    objects.push((
        plan.root_id,
        format!(
            "<< /Count {}{}{} >>\n",
            plan.visible_count,
            first
                .map(|id| format!(" /First {id} 0 R"))
                .unwrap_or_default(),
            last.map(|id| format!(" /Last {id} 0 R"))
                .unwrap_or_default()
        )
        .into_bytes(),
    ));

    let mut node_objects = plan
        .nodes
        .iter()
        .map(|node| {
            (
                node.id,
                outline_node_object(node, first_page_id, document).into_bytes(),
            )
        })
        .collect::<Vec<_>>();
    node_objects.sort_by_key(|(id, _)| *id);
    objects.extend(node_objects);
    objects
}

pub(super) fn outline_node_object(
    node: &OutlineNodePlan,
    first_page_id: usize,
    document: &Document,
) -> String {
    let page_index = node
        .bookmark
        .page_index
        .min(document.pages.len().saturating_sub(1));
    let page_id = first_page_id + page_index;
    format!(
        "<< /Title ({}) /Parent {} 0 R{}{}{}{} /Count {} /Dest [{} 0 R /XYZ {:.3} {:.3} 0] >>\n",
        escape_pdf_string(&node.bookmark.label),
        node.parent_id,
        node.prev_id
            .map(|id| format!(" /Prev {id} 0 R"))
            .unwrap_or_default(),
        node.next_id
            .map(|id| format!(" /Next {id} 0 R"))
            .unwrap_or_default(),
        node.first_child_id
            .map(|id| format!(" /First {id} 0 R"))
            .unwrap_or_default(),
        node.last_child_id
            .map(|id| format!(" /Last {id} 0 R"))
            .unwrap_or_default(),
        node.child_count,
        page_id,
        node.bookmark.x,
        node.bookmark.y
    )
}
