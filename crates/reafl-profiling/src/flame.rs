//! Aggregation of recorded spans into a flame graph.
//!
//! Spans arrive in completion order, so the tree is rebuilt from the recorded
//! nesting (`RawSpan::depth`) after sorting by start time per thread. Threads
//! are aggregated separately and then merged by name, which keeps the drawing
//! simple without losing the per-thread structure.

use std::collections::HashMap;

use crate::span::{Channel, RawSpan};

/// One node of the flame graph.
#[derive(Clone, Debug, Default)]
pub struct FlameNode {
    pub name: &'static str,
    /// Inclusive time: this node plus every child.
    pub total_ns: u64,
    /// Exclusive time: time spent in this node alone.
    pub self_ns: u64,
    /// How many spans were folded into this node.
    pub calls: u32,
    pub children: Vec<FlameNode>,
}

impl FlameNode {
    /// Depth-first list of `(depth, node)`, root first.
    pub fn walk<'a>(&'a self, depth: u16, out: &mut Vec<(u16, &'a FlameNode)>) {
        out.push((depth, self));
        for child in &self.children {
            child.walk(depth + 1, out);
        }
    }

    /// Inclusive time of this node as a fraction of `total`.
    pub fn fraction(&self, total: u64) -> f32 {
        if total == 0 {
            0.0
        } else {
            (self.total_ns as f32 / total as f32).clamp(0.0, 1.0)
        }
    }
}

/// Flame graph for one channel.
#[derive(Clone, Debug, Default)]
pub struct ChannelFlame {
    pub channel: Channel,
    /// Top-level nodes, merged across threads and sorted by time.
    pub roots: Vec<FlameNode>,
    /// Sum of every root's inclusive time.
    pub total_ns: u64,
}

/// Everything the debug panel needs, published roughly twelve times a second.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub channels: Vec<ChannelFlame>,
    /// Length of the history that was folded, in seconds.
    pub window_seconds: f32,
    pub dropped: u64,
}

impl Snapshot {
    /// The flame graph for `channel`, if that channel recorded anything.
    pub fn channel(&self, channel: Channel) -> Option<&ChannelFlame> {
        self.channels.iter().find(|flame| flame.channel == channel)
    }

    /// Slowest top-level span across all channels, used for the summary line.
    pub fn peak(&self) -> Option<&FlameNode> {
        self.channels
            .iter()
            .flat_map(|flame| flame.roots.iter())
            .max_by_key(|node| node.total_ns)
    }
}

/// Folds `spans` into a [`Snapshot`]. Called by the aggregation worker only.
pub(crate) fn build(spans: &[RawSpan], window_seconds: f32, dropped: u64) -> Snapshot {
    // Group by (channel, thread) so each thread's tree is rebuilt from a
    // time-ordered span list.
    let mut groups: HashMap<(Channel, u16), Vec<&RawSpan>> = HashMap::new();
    for span in spans {
        groups
            .entry((span.channel, span.thread))
            .or_default()
            .push(span);
    }

    let mut per_channel: HashMap<Channel, Vec<FlameNode>> = HashMap::new();
    for ((channel, _thread), mut thread_spans) in groups {
        thread_spans.sort_by_key(|span| (span.start_ns, std::cmp::Reverse(span.duration_ns)));
        let roots = build_thread_tree(&thread_spans);
        let merged = per_channel.entry(channel).or_default();
        for root in roots {
            merge_node(merged, root);
        }
    }

    let mut channels: Vec<ChannelFlame> = per_channel
        .into_iter()
        .map(|(channel, mut roots)| {
            finish(&mut roots);
            roots.sort_by(|left, right| right.total_ns.cmp(&left.total_ns));
            let total_ns = roots.iter().map(|node| node.total_ns).sum();
            ChannelFlame {
                channel,
                roots,
                total_ns,
            }
        })
        .collect();
    channels.sort_by_key(|flame| flame.channel);

    Snapshot {
        channels,
        window_seconds,
        dropped,
    }
}

/// Rebuilds one thread's call tree from spans sorted by start time.
fn build_thread_tree(spans: &[&RawSpan]) -> Vec<FlameNode> {
    let mut roots: Vec<FlameNode> = Vec::new();
    // Path of indices from the root to the node that opened last.
    let mut path: Vec<usize> = Vec::new();
    for span in spans {
        let depth = usize::from(span.depth);
        while path.len() > depth {
            path.pop();
        }
        let node = FlameNode {
            name: span.name,
            total_ns: span.duration_ns,
            self_ns: span.duration_ns,
            calls: 1,
            children: Vec::new(),
        };
        let index = append_child(&mut roots, &path, node);
        path.push(index);
    }
    roots
}

/// Adds `node` under the container reached through `path`, returning its index
/// among that container's children.
fn append_child(roots: &mut Vec<FlameNode>, path: &[usize], node: FlameNode) -> usize {
    let mut level = roots;
    for &index in path {
        level = &mut level[index].children;
    }
    level.push(node);
    level.len() - 1
}

/// Adds `node`'s time into a sibling with the same name, or appends it.
fn merge_node(level: &mut Vec<FlameNode>, node: FlameNode) {
    if let Some(existing) = level.iter_mut().find(|sibling| sibling.name == node.name) {
        existing.total_ns += node.total_ns;
        existing.self_ns += node.self_ns;
        existing.calls += node.calls;
        for child in node.children {
            merge_node(&mut existing.children, child);
        }
        return;
    }
    level.push(node);
}

/// Recomputes exclusive time bottom-up and sorts children by time.
fn finish(level: &mut [FlameNode]) {
    for node in level {
        finish(&mut node.children);
        node.children
            .sort_by(|left, right| right.total_ns.cmp(&left.total_ns));
        let child_total: u64 = node.children.iter().map(|child| child.total_ns).sum();
        node.self_ns = node.total_ns.saturating_sub(child_total.min(node.total_ns));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(name: &'static str, start: u64, duration: u64, depth: u16) -> RawSpan {
        RawSpan {
            name,
            start_ns: start,
            duration_ns: duration,
            depth,
            channel: Channel::Ui,
            thread: 1,
        }
    }

    #[test]
    fn nesting_rebuilds_from_depth() {
        // frame(0..100) > paint(10..60) > text(20..30)
        let spans = [
            span("text", 20, 10, 2),
            span("paint", 10, 50, 1),
            span("frame", 0, 100, 0),
        ];
        let snapshot = build(&spans, 1.0, 0);
        let flame = snapshot.channel(Channel::Ui).unwrap();
        assert_eq!(flame.roots.len(), 1);
        let frame = &flame.roots[0];
        assert_eq!(frame.name, "frame");
        assert_eq!(frame.total_ns, 100);
        assert_eq!(frame.self_ns, 50);
        assert_eq!(frame.children[0].name, "paint");
        assert_eq!(frame.children[0].children[0].name, "text");
    }

    #[test]
    fn same_name_siblings_are_merged() {
        let spans = [
            span("frame", 0, 10, 0),
            span("frame", 20, 30, 0),
            span("frame", 60, 10, 0),
        ];
        let snapshot = build(&spans, 1.0, 0);
        let flame = snapshot.channel(Channel::Ui).unwrap();
        assert_eq!(flame.roots.len(), 1);
        assert_eq!(flame.roots[0].total_ns, 50);
        assert_eq!(flame.roots[0].calls, 3);
    }

    #[test]
    fn empty_input_is_empty_snapshot() {
        let snapshot = build(&[], 2.0, 7);
        assert!(snapshot.channels.is_empty());
        assert_eq!(snapshot.dropped, 7);
        assert!(snapshot.peak().is_none());
    }
}
