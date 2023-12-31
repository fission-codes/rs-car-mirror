use bytes::Bytes;
use libipld::{Cid, Ipld, IpldCodec};
use libipld_core::multihash::{Code, MultihashDigest};
use proptest::{
    prelude::{Rng, RngCore},
    strategy::Strategy,
    test_runner::TestRng,
};
use roaring_graphs::{arb_dag, DirectedAcyclicGraph, Vertex};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Debug,
    ops::Range,
};
use wnfs_common::encode;

/// A strategy for use with proptest to generate random DAGs (directed acyclic graphs).
/// The strategy generates a list of blocks of type T and their CIDs, as well as
/// the root block's CID.
pub fn arb_ipld_dag<T: Debug + Clone>(
    vertex_count: impl Into<Range<Vertex>>,
    edge_probability: f64,
    generate_block: impl Fn(Vec<Cid>, &mut TestRng) -> (Cid, T) + Clone,
) -> impl Strategy<Value = (Vec<(Cid, T)>, Cid)> {
    arb_dag(vertex_count, edge_probability)
        .prop_perturb(move |dag, mut rng| dag_to_nodes(&dag, &mut rng, generate_block.clone()))
}

/// A block-generating function for use with `arb_ipld_dag`.
pub fn links_to_ipld(cids: Vec<Cid>, _: &mut TestRng) -> (Cid, Ipld) {
    let ipld = Ipld::List(cids.into_iter().map(Ipld::Link).collect());
    let bytes = encode(&ipld, IpldCodec::DagCbor).unwrap();
    let cid = Cid::new_v1(IpldCodec::DagCbor.into(), Code::Blake3_256.digest(&bytes));
    (cid, ipld)
}

/// A block-generating function for use with `arb_ipld_dag`.
pub fn links_to_dag_cbor(cids: Vec<Cid>, _: &mut TestRng) -> (Cid, Bytes) {
    let ipld = Ipld::List(cids.into_iter().map(Ipld::Link).collect());
    let bytes: Bytes = encode(&ipld, IpldCodec::DagCbor).unwrap().into();
    let cid = Cid::new_v1(IpldCodec::DagCbor.into(), Code::Blake3_256.digest(&bytes));
    (cid, bytes)
}

/// A block-generating function for use with `arb_ipld_dag`.
///
/// Creates (a function that creates) an IPLD block with given links & some
/// random `padding_bytes` bytes attached.
pub fn links_to_padded_ipld(
    padding_bytes: usize,
) -> impl Fn(Vec<Cid>, &mut TestRng) -> (Cid, Ipld) + Clone {
    move |cids, rng| {
        let mut padding = vec![0u8; padding_bytes];
        rng.fill_bytes(&mut padding);

        let codec = match rng.gen_bool(0.5) {
            true if cids.is_empty() => IpldCodec::Raw,
            _ => IpldCodec::DagCbor,
        };

        let ipld = if cids.is_empty() && codec == IpldCodec::Raw {
            Ipld::Bytes(padding)
        } else {
            Ipld::Map(BTreeMap::from([
                ("data".into(), Ipld::Bytes(padding)),
                (
                    "links".into(),
                    Ipld::List(cids.into_iter().map(Ipld::Link).collect()),
                ),
            ]))
        };

        let bytes = encode(&ipld, codec).unwrap();
        let cid = Cid::new_v1(codec.into(), Code::Blake3_256.digest(&bytes));
        (cid, ipld)
    }
}

/// Turn a directed acyclic graph into a list of nodes (with their CID) and a root CID.
/// This will select only the DAG that's reachable from the root.
pub fn dag_to_nodes<T>(
    dag: &DirectedAcyclicGraph,
    rng: &mut TestRng,
    generate_node: impl Fn(Vec<Cid>, &mut TestRng) -> (Cid, T) + Clone,
) -> (Vec<(Cid, T)>, Cid) {
    let mut blocks = Vec::new();
    let mut visited = HashSet::new();
    let (cid, block) = dag_to_nodes_helper(dag, 0, rng, generate_node, &mut blocks, &mut visited);
    blocks.push((cid, block));
    (blocks, cid)
}

fn dag_to_nodes_helper<T>(
    dag: &DirectedAcyclicGraph,
    root: Vertex,
    rng: &mut TestRng,
    generate_node: impl Fn(Vec<Cid>, &mut TestRng) -> (Cid, T) + Clone,
    arr: &mut Vec<(Cid, T)>,
    visited: &mut HashSet<Vertex>,
) -> (Cid, T) {
    let mut child_blocks = Vec::new();
    if root >= dag.get_vertex_count() {
        println!("{root}, {}", dag.get_vertex_count());
    }
    for child in dag.iter_children(root) {
        if visited.contains(&child) {
            continue;
        }
        visited.insert(child);
        child_blocks.push(dag_to_nodes_helper(
            dag,
            child,
            rng,
            generate_node.clone(),
            arr,
            visited,
        ));
    }
    let result = generate_node(child_blocks.iter().map(|(cid, _)| *cid).collect(), rng);
    arr.extend(child_blocks);
    result
}
