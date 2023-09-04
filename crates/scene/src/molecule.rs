use std::collections::{BTreeMap, HashMap};

use lazy_static::lazy_static;
use periodic_table::Element;
use petgraph::{stable_graph, visit::IntoNodeReferences};
use render::{AtomKind, AtomRepr, Atoms, GlobalRenderResources};
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
pub type MoleculeCheckpoint = HashMap<AtomSpecifier, Vec3>;
pub type BondOrder = u8;
pub type AtomIndex = stable_graph::NodeIndex;
#[allow(unused)]
pub type BondIndex = stable_graph::EdgeIndex;

pub struct AtomNode {
    pub element: Element,
    pub raw_pos: Vec3,
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
                let head = commands
                    .find_atom(head)
                    .expect("The atom specifier an atom that is bonded should exist");

                (head.raw_pos - self.raw_pos).normalized()
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
    corrections: MoleculeCheckpoint,
}

impl MoleculeRepr {
    fn atom_reprs(&self) -> Vec<AtomRepr> {
        self.graph
            .node_weights()
            .map(|node| AtomRepr {
                kind: AtomKind::new(node.element),
                pos: self
                    .pos(&node.spec)
                    .expect("Every atom in the graph should have a correction"),
            })
            .collect()
    }

    fn clear(&mut self) {
        self.atom_map.clear();
        self.graph.clear();
        self.bounding_box = Default::default();
        self.gpu_synced = false;
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

        self.gpu_synced = true;
    }

    pub fn atoms(&self) -> Option<&Atoms> {
        self.gpu_atoms.as_ref()
    }

    pub fn compute_corrections(&mut self, target_positions: &HashMap<AtomSpecifier, Vec3>) {
        for atom in self.graph.node_weights() {
            let new_position = target_positions
                .get(&atom.spec)
                .expect("A map of target positions should contain every atom's position.");
            let correction = *new_position - atom.raw_pos;
            self.corrections.insert(atom.spec.clone(), correction);
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
            raw_pos: pos,
            spec: spec.clone(),
            head,
        });

        self.atom_map.insert(spec.clone(), index);
        self.corrections.insert(spec, Default::default());
        self.bounding_box.enclose_sphere(
            pos,
            // TODO: This is
            PERIODIC_TABLE.element_reprs[element as usize].radius,
        );
        self.gpu_synced = false;

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

    fn pos(&self, spec: &AtomSpecifier) -> Option<Vec3> {
        if let Some(&correction) = self.corrections.get(spec) {
            return Some(self.find_atom(spec)?.raw_pos + correction);
        }

        None
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
#[serde_as]
#[derive(Serialize)]
pub struct Molecule {
    #[serde(skip)]
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
    // checkpoints: HashMap<usize, MoleculeCheckpoint>,
    // TODO: this is just a shim. In the future, we want to have a map of history steps to checkpoints,
    // and then use them to make feature reconstruction much faster. For now, we just store the currrent
    // checkpoint. For reference, this `checkpoint` is just a clone of repr.corrections.
    //
    // To properly address this, we need to perform
    #[serde_as(as = "Vec<(_, _)>")]
    pub active_checkpoint: HashMap<AtomSpecifier, Vec3>, // When checkpointing is implemented, this will be needed:
                                                         //
                                                         // the history step we cannot equal or exceed without first recomputing. For example, if repr
                                                         // is up to date with the feature list, and then a past feature is changed, dirty_step would change
                                                         // from `features.len()` to the index of the changed feature. This is used to determine if recomputation
                                                         // is needed when moving forwards in the timeline, or if a future checkpoint can be used.
                                                         // dirty_step: usize,
}

impl Molecule {
    pub fn from_feature(feature: Feature) -> Self {
        let mut repr = MoleculeRepr::default();
        feature
            .apply(&0, &mut repr)
            .expect("Primitive features should never return a feature error!");
        let mut features = FeatureList::default();
        features.push_back(feature);
        let active_checkpoint = repr.corrections.clone();

        Self {
            repr,
            rotation: ultraviolet::Rotor3::default(),
            offset: ultraviolet::Vec3::default(),
            features,
            history_step: 1, // This starts at 1 because we applied the primitive feature
            active_checkpoint,
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

        // If we are stepping back, we need to recompute starting at the beginning
        // (we don't currently use checkpoints or feature inversion).
        if history_step < self.history_step {
            self.history_step = 0;
            self.repr.clear();
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

            let new_positions = crate::dynamics::relax(&self.repr.graph, &self.repr, 0.01);
            self.repr.compute_corrections(&new_positions);
            self.active_checkpoint = self.repr.corrections.clone();
        }

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
            for atom_index in graph.node_indices() {
                let atom = graph.node_weight(atom_index).expect("Iterating over an immutably referenced graph should always provide valid node indexes");
                let atom_radius_sq = PERIODIC_TABLE.element_reprs[atom.element as usize]
                    .radius
                    .powi(2);

                if (current_pos - atom.raw_pos).mag_sq() < atom_radius_sq {
                    return Some(atom.spec.clone());
                }
            }

            current_pos += step;
        }

        None
    }
}

impl<'de> Deserialize<'de> for Molecule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // This is all the data that a Molecule serializes into. The other
        // fields on molecule are large in size and easy to recompute, so we will
        // use the raw molecule representation to reconstruct them.
        #[serde_as]
        #[derive(Deserialize)]
        struct RawMolecule {
            rotation: ultraviolet::Rotor3,
            offset: ultraviolet::Vec3,
            features: FeatureList,
            history_step: usize,
            #[serde_as(as = "Vec<(_, _)>")]
            active_checkpoint: HashMap<AtomSpecifier, Vec3>,
        }

        // TODO: integrity check of the deserialized struct

        let raw_molecule = RawMolecule::deserialize(deserializer)?;

        let mut molecule = Molecule {
            repr: MoleculeRepr::default(),
            rotation: raw_molecule.rotation,
            offset: raw_molecule.offset,
            features: raw_molecule.features,
            history_step: 0, // This starts at 0 because we haven't applied the features, we've just loaded them
            active_checkpoint: raw_molecule.active_checkpoint,
        };

        // this advances the history step to the correct location
        molecule.set_history_step(raw_molecule.history_step);

        Ok(molecule)
    }
}
