use std::collections::HashMap;

use lazy_static::lazy_static;
use periodic_table::Element;
use petgraph::{
    stable_graph,
    visit::{IntoEdgeReferences, IntoEdges, IntoNodeReferences},
};
use render::{AtomKind, AtomRepr, Atoms, BondRepr, Bonds, GlobalRenderResources};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use ultraviolet::Vec3;

use crate::{
    feature::{Feature, FeatureError, FeatureList, MoleculeCommands, ReferenceType},
    ids::AtomSpecifier,
    utils::BoundingBox,
};

pub type MoleculeGraph = stable_graph::StableUnGraph<AtomNode, BondOrder>;
// A map that gives each atom in a molecule a coordinate. Used to cache structure energy minimization
// calculations
pub type AtomPositions = HashMap<AtomSpecifier, Vec3>;
pub type BondOrder = u8;
pub type AtomIndex = stable_graph::NodeIndex;
#[allow(unused)]
pub type BondIndex = stable_graph::EdgeIndex;

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
pub struct MoleculeCheckpoint {
    graph: MoleculeGraph,
    #[serde_as(as = "Vec<(_, _)>")]
    positions: AtomPositions,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AtomNode {
    pub element: Element,
    pub spec: AtomSpecifier,
    // The atom that this atom was bonded to (and uses as a "forward" direction). If
    // no such atom exists, then this atom is the root atom, and the forward direction
    // should be taken to be the molecule's +z axis. Although this field is not yet
    // used (as of september 3rd 2023), it is needed to describe molecular geometry
    // in terms of bond angles and lengths (which will be useful later on).
    pub head: Option<AtomSpecifier>,
}

impl AtomNode {
    pub fn forward(&self, commands: &dyn MoleculeCommands) -> Vec3 {
        match self.head {
            Some(ref head) => {
                let head_pos = commands
                    .pos(head)
                    .expect("The atom specifier an atom that is bonded should exist");
                let pos = commands
                    .pos(&self.spec)
                    .expect("The atom specifier an atom that is bonded should exist");

                (*head_pos - *pos).normalized()
            }
            None => Vec3::unit_z(),
        }
    }
}

/// The concrete representation of the molecule at some time in the feature history.
#[derive(Default)]
pub struct MoleculeRepr {
    // TODO: This atom map is a simple but extremely inefficient implementation. This data
    // is highly structued and repetitive: compression, flattening, and a tree could do
    // a lot to optimize this.
    atom_map: HashMap<AtomSpecifier, AtomIndex>,
    pub graph: MoleculeGraph,
    bounding_box: BoundingBox,
    gpu_synced: bool,
    gpu_atoms: Option<Atoms>,
    gpu_bonds: Option<Bonds>,
    positions: AtomPositions,
}

impl MoleculeRepr {
    fn atom_reprs(&self) -> Vec<AtomRepr> {
        self.graph
            .node_weights()
            .map(|node| AtomRepr {
                kind: AtomKind::new(node.element),
                pos: *self
                    .pos(&node.spec)
                    .expect("Every atom in the graph should have a position"),
            })
            .collect()
    }

    fn bond_reprs(&self) -> Vec<BondRepr> {
        self.graph
            .edge_indices()
            .map(|edge_idx| {
                // get the atom positions that straddle the edge
                let atom_positions = {
                    let (i1, i2) = self.graph.edge_endpoints(edge_idx).unwrap();
                    [i1, i2].map(|i| {
                        let spec = &self.graph.node_weight(i).unwrap().spec;
                        *self.pos(spec).unwrap()
                    })
                };

                BondRepr {
                    start_pos: atom_positions[0],
                    end_pos: atom_positions[0],
                    order: *self.graph.edge_weight(edge_idx).unwrap() as u32,
                    pad: 0,
                }
            })
            .collect()
    }

    fn clear(&mut self) {
        self.atom_map.clear();
        self.graph.clear();
        self.bounding_box = Default::default();
        self.gpu_synced = false;
    }

    pub(crate) fn relax(&mut self) {
        self.positions = crate::dynamics::relax(&self.graph, &self.positions, 0.01);
    }

    pub fn reupload_atoms(&mut self, gpu_resources: &GlobalRenderResources) {
        // TODO: not working, see shinzlet/atomCAD #3
        // self.gpu_atoms.reupload_atoms(&atoms, gpu_resources);

        // This is a workaround, but it has bad perf as it always drops and
        // reallocates

        if self.graph.node_count() == 0 {
            self.gpu_atoms = None;
        } else {
            self.gpu_atoms = Some(Atoms::new(gpu_resources, self.atom_reprs()));
        }

        if self.graph.edge_count() == 0 {
            self.gpu_bonds = None;
        } else {
            self.gpu_bonds = Some(Bonds::new(gpu_resources, self.bond_reprs()));
        }

        self.gpu_synced = true;
    }

    pub fn atoms(&self) -> Option<&Atoms> {
        self.gpu_atoms.as_ref()
    }

    pub fn bonds(&self) -> Option<&Bonds> {
        self.gpu_bonds.as_ref()
    }

    pub fn set_checkpoint(&mut self, checkpoint: MoleculeCheckpoint) {
        self.graph = checkpoint.graph;
        self.positions = checkpoint.positions;
        self.atom_map.clear();

        for (atom_index, atom) in self.graph.node_references() {
            self.atom_map.insert(atom.spec.clone(), atom_index);
        }
    }

    pub fn make_checkpoint(&self) -> MoleculeCheckpoint {
        MoleculeCheckpoint {
            graph: self.graph.clone(),
            positions: self.positions.clone(),
        }
    }
}

lazy_static! {
    pub static ref PERIODIC_TABLE: periodic_table::PeriodicTable =
        periodic_table::PeriodicTable::new();
}

impl MoleculeCommands for MoleculeRepr {
    fn add_bonded_atom(
        &mut self,
        element: Element,
        pos: ultraviolet::Vec3,
        spec: AtomSpecifier,
        bond_target: AtomSpecifier,
        bond_order: BondOrder,
    ) -> Result<(), FeatureError> {
        self.add_atom(element, pos, spec.clone(), Some(bond_target.clone()))?;
        self.create_bond(&spec, &bond_target, bond_order)
    }

    fn add_atom(
        &mut self,
        element: Element,
        pos: ultraviolet::Vec3,
        spec: AtomSpecifier,
        head: Option<AtomSpecifier>,
    ) -> Result<(), FeatureError> {
        if self.atom_map.contains_key(&spec) {
            return Err(FeatureError::AtomOverwrite);
        }

        let index = self.graph.add_node(AtomNode {
            element,
            spec: spec.clone(),
            head,
        });

        self.atom_map.insert(spec.clone(), index);
        self.bounding_box.enclose_sphere(
            pos,
            // TODO: This is
            PERIODIC_TABLE.element_reprs[element as usize].radius,
        );
        self.gpu_synced = false;
        self.positions.insert(spec, pos);

        Ok(())
    }

    fn create_bond(
        &mut self,
        a1: &AtomSpecifier,
        a2: &AtomSpecifier,
        order: BondOrder,
    ) -> Result<(), FeatureError> {
        match (self.atom_map.get(a1), self.atom_map.get(a2)) {
            (Some(&a1_index), Some(&a2_index)) => {
                self.graph.add_edge(a1_index, a2_index, order);
                Ok(())
            }
            _ => Err(FeatureError::BrokenReference(ReferenceType::Atom)),
        }
    }

    fn find_atom(&self, spec: &AtomSpecifier) -> Option<&AtomNode> {
        match self.atom_map.get(spec) {
            Some(atom_index) => self.graph.node_weight(*atom_index),
            None => None,
        }
    }

    fn pos(&self, spec: &AtomSpecifier) -> Option<&Vec3> {
        self.positions.get(spec)
    }
}

/// Demonstration of how to use the feature system
/// let mut molecule = Molecule::from_feature(
///     &gpu_resources,
///     RootAtom {
///         element: Element::Iodine,
///     },
/// );
///
/// molecule.push_feature(AtomFeature {
///     target: scene::ids::AtomSpecifier::new(0),
///     element: Element::Sulfur,
/// });
/// molecule.apply_all_features();
///
/// molecule.push_feature(AtomFeature {
///     target: scene::ids::AtomSpecifier::new(1),
///     element: Element::Carbon,
/// });
/// molecule.apply_all_features();
///
/// molecule.set_history_step(2);
/// molecule.reupload_atoms(&gpu_resources);
pub struct Molecule {
    pub repr: MoleculeRepr,
    #[allow(unused)]
    rotation: ultraviolet::Rotor3,
    #[allow(unused)]
    offset: ultraviolet::Vec3,
    features: FeatureList,
    // The index one greater than the most recently applied feature's location in the feature list.
    // This is unrelated to feature IDs: it is effectively just a counter of how many features are
    // applied. (i.e. our current location in the edit history timeline)
    history_step: usize,
    // when `history_step` is set to `i`, if `checkpoints` contains the key `i`, then
    // `checkpoints.get(i)` contains the graph and geometry which should be used to render
    // the molecule. This allows feature application and relaxation to be cached until they
    // need to be recomputed. This saves a lot of time, as relaxation is a very expensive operation that does not
    // commute with feature application.
    checkpoints: HashMap<usize, MoleculeCheckpoint>,
    // the history step we cannot equal or exceed without first recomputing. For example, if repr
    // is up to date with the feature list, and then a past feature is changed, dirty_step would change
    // from `features.len()` to the index of the changed feature. This is used to determine if recomputation
    // is needed when moving forwards in the timeline, or if a future checkpoint can be used.
    dirty_step: usize,
}

impl Molecule {
    pub fn from_feature(feature: Feature) -> Self {
        let mut repr = MoleculeRepr::default();
        feature
            .apply(&0, &mut repr)
            .expect("Primitive features should never return a feature error!");
        repr.relax();

        let mut features = FeatureList::default();
        features.push_back(feature);

        Self {
            repr,
            rotation: ultraviolet::Rotor3::default(),
            offset: ultraviolet::Vec3::default(),
            features,
            history_step: 1, // This starts at 1 because we applied the primitive feature
            checkpoints: Default::default(),
            dirty_step: 1, // Although no checkpoints exist, repr is not dirty, so we advance this to its max
        }
    }

    pub fn features(&self) -> &FeatureList {
        &self.features
    }

    pub fn push_feature(&mut self, feature: Feature) {
        self.features.insert(feature, self.history_step);
    }

    // Advances the model to a given history step by applying features in the timeline.
    // This will not in general recompute the history, so if a past feature is changed,
    // you must recompute from there.
    pub fn set_history_step(&mut self, history_step: usize) {
        // TODO: Bubble error to user
        assert!(
            history_step <= self.features.len(),
            "history step exceeds feature list size"
        );

        // Find the best checkpoint to start reconstructing from:
        let best_checkpoint = self
            .checkpoints
            .keys()
            .filter(|candidate| **candidate <= history_step)
            .max();

        match best_checkpoint {
            None => {
                // If there wasn't a usable checkpoint, we can either keep computing forwards or
                // restart. We only have to restart from scratch if we're moving backwards, otherwise
                // we can just move forwards.

                if self.history_step > history_step {
                    self.history_step = 0;
                    self.repr.clear();
                }
            }
            Some(best_checkpoint) => {
                // If there was, we can go there and resume from that point
                self.repr
                    .set_checkpoint(self.checkpoints.get(best_checkpoint).unwrap().clone());
                self.history_step = *best_checkpoint;
            }
        }

        for feature_id in &self.features.order()[self.history_step..history_step] {
            println!("Applying feature {}", feature_id);
            let feature = self
                .features
                .get(feature_id)
                .expect("Feature IDs referenced by the FeatureList order should exist!");

            if feature.apply(feature_id, &mut self.repr).is_err() {
                // TODO: Bubble error to the user
                println!("Feature reconstruction error on feature {}", feature_id);
                dbg!(&feature);
            }

            self.repr.relax();
        }

        self.dirty_step = history_step;
        self.history_step = history_step;
    }

    // equivalent to `set_history_step(features.len()): applies every feature that is in the
    // feature timeline.
    pub fn apply_all_features(&mut self) {
        self.set_history_step(self.features.len())
    }

    // TODO: Optimize heavily (use octree, compute entry point of ray analytically)
    pub fn get_ray_hit(&self, origin: Vec3, direction: Vec3) -> Option<AtomSpecifier> {
        // Using `direction` as a velocity vector, determine when the ray will
        // collide with the bounding box. Note the ? - this fn returns early if there
        // isn't a collision.
        let (tmin, tmax) = self.repr.bounding_box.ray_hit_times(origin, direction)?;

        // If the box is fully behind the raycast direction, we will never get a hit.
        if tmax <= 0.0 {
            return None;
        }

        // Knowing that the ray will enter the box, we can now march along it by a fixed step
        // size. At each step, we check for a collision with an atom, and return that atom's index
        // if a collision occurs.

        // We know that the box is first hit at `origin + tmin * direction`. However,
        // tmin can be negative, and we only want to march forwards. So,
        // we constrain tmin to be nonnegative.
        let mut current_pos = origin + f32::max(0.0, tmin) * direction;

        // This is an empirically reasonable value. It is still possible to miss an atom if
        // the user clicks on the very edge of it, but this is rare.
        let step_size = PERIODIC_TABLE.element_reprs[Element::Hydrogen as usize].radius / 10.0;
        let step = direction * step_size;
        let t_span = tmax - f32::max(0.0, tmin);
        // the direction vector is normalized, so 1 unit of time = 1 unit of space
        let num_steps = (t_span / step_size) as usize;

        let graph = &self.repr.graph;
        for _ in 0..num_steps {
            for atom in graph.node_weights() {
                let atom_radius_sq = PERIODIC_TABLE.element_reprs[atom.element as usize]
                    .radius
                    .powi(2);

                let atom_pos = *self
                    .repr
                    .positions
                    .get(&atom.spec)
                    .expect("Every atom in the graph should have an associated position");
                if (current_pos - atom_pos).mag_sq() < atom_radius_sq {
                    return Some(atom.spec.clone());
                }
            }

            current_pos += step;
        }

        None
    }
}

// This is a stripped down representation of the molecule that removes several
// fields (some are redundant, like repr.atom_map, and some are not serializable,
// like repr.gpu_atoms).
#[derive(Serialize, Deserialize)]
struct ProxyMolecule {
    rotation: ultraviolet::Rotor3,
    offset: ultraviolet::Vec3,
    features: FeatureList,
    history_step: usize,
    checkpoints: HashMap<usize, MoleculeCheckpoint>,
    dirty_step: usize,
}

impl Serialize for Molecule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Custom serialization is used to ensure that the current molecule state is
        // saved as a checkpoint, even if it normally would not be (i.e. if it's already
        // very close to an existing checkpoint). This allows faster loading when the file
        // is reopened.

        let mut checkpoints = self.checkpoints.clone();
        checkpoints.insert(self.history_step, self.repr.make_checkpoint());

        let data = ProxyMolecule {
            rotation: self.rotation,
            offset: self.offset,
            features: self.features.clone(),
            history_step: self.history_step,
            checkpoints,
            dirty_step: self.dirty_step,
        };

        data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Molecule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // TODO: integrity check of the deserialized struct

        let data = ProxyMolecule::deserialize(deserializer)?;

        let mut molecule = Molecule {
            repr: MoleculeRepr::default(),
            rotation: data.rotation,
            offset: data.offset,
            features: data.features,
            history_step: data.history_step, // This starts at 0 because we haven't applied the features, we've just loaded them

            checkpoints: data.checkpoints,
            dirty_step: data.dirty_step,
        };

        // this advances the history step to the correct location
        molecule.set_history_step(data.history_step);

        Ok(molecule)
    }
}
