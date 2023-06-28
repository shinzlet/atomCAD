// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/*
 * This shader uses an interesting technique that is sometimes
 * called progrmamable vertex pulling.
 *
 * Instead of providing a vertex buffer, we instead provide an array
 * of points in a storage buffer and then call draw with <number of points> * 6
 *
 * This is apparently faster than instancing for small meshes.
 *
 * See slide 20 of https://www.slideshare.net/DevCentralAMD/vertex-shader-tricks-bill-bilodeau.
 */
#version 450

layout(set = 0, binding = 0) uniform Camera {
    vec4 projection[4];
    vec4 view[4];
    vec4 projection_view[4];
} camera;

struct Element {
    vec3 color;
    float radius;
};

layout(set = 0, binding = 1) readonly buffer PeriodicTable {
    Element elements[118];
} periodic_table;

struct Atom {
    vec3 pos;
    uint kind;
};

// Should be marked 'readonly', but this is causing wgpu to barf.  Maybe this
// is fixed upstream in later releases?
layout(set = 1, binding = 1, std430) buffer Atoms {
    uvec2 fragment_id; // high and low

    Atom atoms[]; // this must be aligned to 16 bytes.
};

// struct Bivec {
//     float xy;
//     float xz;
//     float yz;
// }

// struct Rotor {
//     float s;
//     Bivec bv;
// };

layout(location = 0) in vec4 part_fragment_transform_0; // fragement * part transformation
layout(location = 1) in vec4 part_fragment_transform_1;
layout(location = 2) in vec4 part_fragment_transform_2;
layout(location = 3) in vec4 part_fragment_transform_3;

layout(location = 0) out vec2 uv;
layout(location = 1) out vec4 position_clip_space;
layout(location = 2) flat out vec4 element_vec;
layout(location = 4) flat out vec4 center_view_space;
layout(location = 5) out vec4 position_view_space;

const vec2 vertices[3] = {
    vec2(1.73, -1.0),
    vec2(-1.73, -1.0),
    vec2(0.0, 2.0)
};

// vec3 rotate_by_rotor(Rotor rotor, vec3 point) {
//     const float fx = rotor.s * point.x + rotor.bv.xy * point.y + rotor.bv.xz * point.z;
//     const float fy = rotor.s * point.y - rotor.bv.xy * point.x + rotor.bv.yz * point.z;
//     const float fz = rotor.s * point.z - rotor.bv.xz * point.x - rotor.bv.yz * point.y;
//     const float fw = rotor.bv.xy * point.z - rotor.bv.xz * point.y + rotor.bv.yz * point.x;

//     return vec3(
//         rotor.s * fx + rotor.bv.xy * fy + rotor.bv.xz * fz + rotor.bv.yz * fw,
//         rotor.s * fy - rotor.bv.xy * fx - rotor.bv.xz * fw + rotor.bv.yz * fz,
//         rotor.s * fz + rotor.bv.xy * fw - rotor.bv.xz * fx - rotor.bv.yz * fy,
//     );
// }

out gl_PerVertex {
    vec4 gl_Position;
};

void main(void) {
    const Atom atom = atoms[gl_VertexIndex / 3];
    Element element = periodic_table.elements[atom.kind & 0x7f];
    element_vec = vec4(element.color, element.radius);
    const vec2 vertex = element.radius * vertices[gl_VertexIndex % 3];

    const mat4 part_fragment_transform = mat4(
        part_fragment_transform_0,
        part_fragment_transform_1,
        part_fragment_transform_2,
        part_fragment_transform_3
    );
    const vec4 position = part_fragment_transform * vec4(atom.pos, 1.0);

    const vec3 camera_right_worldspace = vec3(camera.view[0][0], camera.view[1][0], camera.view[2][0]);
    const vec3 camera_up_worldspace = vec3(camera.view[0][1], camera.view[1][1], camera.view[2][1]);
    const vec4 position_worldspace = vec4(
        position.xyz +
        vertex.x * camera_right_worldspace +
        vertex.y * camera_up_worldspace,
        1.0
    );

    const mat4 camera_projection_view = mat4(
        camera.projection_view[0],
        camera.projection_view[1],
        camera.projection_view[2],
        camera.projection_view[3]
    );
    position_clip_space = camera_projection_view * position_worldspace;
    // position_clip_space = camera_projection_view * position_worldspace;
    uv = vertex;
    // sphere_radius = element.radius;
    const mat4 camera_view = mat4(
        camera.view[0],
        camera.view[1],
        camera.view[2],
        camera.view[3]
    );
    center_view_space = camera_view * vec4(atom.pos, 0.0);
    position_view_space = camera_view * position_worldspace;
    gl_Position = position_clip_space;
}

// End of File
