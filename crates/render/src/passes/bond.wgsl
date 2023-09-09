// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// This shader uses an interesting technique that is sometimes called
// progrmamable vertex pulling.
//
// Instead of providing a vertex buffer, we instead provide an array of points
// in a storage buffer and then call draw with <number of points> * 6
//
// This is apparently faster than instancing for small meshes.
//
// See slide 20 of
// https://www.slideshare.net/DevCentralAMD/vertex-shader-tricks-bill-bilodeau.

struct Camera {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    projection_view: mat4x4<f32>,
};

struct Bond {
    start_pos: vec3<f32>,
    end_pos: vec3<f32>,
    order: u32,
};

@group(0) @binding(0)
var<uniform> camera: Camera;
@group(0) @binding(1)
var<storage> vertices: array<vec2<f32>, 3>;
// @group(0) @binding(2)
// var<storage> periodic_table: PeriodicTable;

@group(1) @binding(0)
var<storage> bonds: array<Bond>;

struct BondVertexInput {
    @builtin(vertex_index)
    index: u32,
    @location(0)
    part_fragment_transform_0: vec4<f32>,
    @location(1)
    part_fragment_transform_1: vec4<f32>,
    @location(2)
    part_fragment_transform_2: vec4<f32>,
    @location(3)
    part_fragment_transform_3: vec4<f32>,
};

struct BondVertexOutput {
    @builtin(position)
    position: vec4<f32>,
    @location(0)
    uv: vec2<f32>,
    @location(1)
    position_clip_space: vec4<f32>,
    @location(2)
    position_view_space: vec4<f32>,
    @location(3) @interpolate(flat)
    center_view_space: vec4<f32>,
};

@vertex
fn vs_main(in: BondVertexInput) -> BondVertexOutput {
    let bond = bonds[in.index / 3u];
    let vertex = vertices[in.index % 3u];

    let part_fragment_transform = mat4x4<f32>(
        in.part_fragment_transform_0,
        in.part_fragment_transform_1,
        in.part_fragment_transform_2,
        in.part_fragment_transform_3
    );
    let pos = (bond.start_pos + bond.end_pos) / 2.0;
    let position = part_fragment_transform * vec4<f32>(pos, 1.0);

    let camera_right_worldspace = vec3<f32>(camera.view[0][0], camera.view[1][0], camera.view[2][0]);
    let camera_up_worldspace = vec3<f32>(camera.view[0][1], camera.view[1][1], camera.view[2][1]);
    let position_worldspace = vec4<f32>(
        position.xyz +
        vertex.x * camera_right_worldspace +
        vertex.y * camera_up_worldspace,
        1.0
    );

    let position_clip_space = camera.projection_view * position_worldspace;
    let center_view_space = camera.view * vec4<f32>(pos, 0.0);
    let position_view_space = camera.view * position_worldspace;

    return BondVertexOutput(position_clip_space, vertex, position_clip_space, position_view_space, center_view_space);
}

alias BondFragmentInput = BondVertexOutput;

struct BondFragmentOutput {
    @builtin(frag_depth)
    depth: f32,
    @location(0)
    color: vec4<f32>,
    @location(1)
    normal: vec4<f32>,
}

fn map(value: f32, low1: f32, high1: f32, low2: f32, high2: f32) -> f32 {
    return low2 + (value - low1) * (high2 - low2) / (high1 - low1);
}

@fragment
fn fs_main(in: BondFragmentInput) -> BondFragmentOutput {
    // return BondFragmentOutput(0.0, vec4(1.0, 0.0, 0.0, 1.0), vec4(1.0, 0.0, 0.0, 1.0));
    let dist = length(in.uv);
    if (dist > 1.0) {
        discard;
    }

    let z = sqrt(1.0 - dist * dist);
    let in_pos_clipspace = in.position_clip_space + camera.projection[2] * z;

    let depth = in_pos_clipspace.z / in_pos_clipspace.w;

    let color = vec4(
        vec3(1.0, 0.0, 0.0) * map(z, 0.0, 1.0, 0.25, 1.0),
        1.0
    );
    let normal = vec4(normalize(in.position_view_space.xyz - in.center_view_space.xyz), 0.0);

    return BondFragmentOutput(depth, color, normal);
}

// End of File
