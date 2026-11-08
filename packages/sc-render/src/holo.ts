// The hologram look: one colour, lit by a fixed light in view space, a
// glowing rim where the surface turns away from the viewer, more opaque at
// the rim than face-on. Works on plain and instanced meshes and honours
// clipping planes (the slice).
import * as THREE from 'three';

export const HOLO_HULL = 0x4fc3f7;
export const HOLO_EDGE = 0x9fe9ff;
export const HOLO_BACKGROUND = 0x08151f;
export const HOLO_GRID = [0x2a6f8f, 0x12303f] as const;

const vertexShader = /* glsl */ `
#include <common>
#include <clipping_planes_pars_vertex>
varying vec3 vNormalView;
varying vec3 vToEye;
void main() {
	#include <begin_vertex>
	#include <project_vertex>
	#include <clipping_planes_vertex>
	vec3 n = normal;
	#ifdef USE_INSTANCING
		n = mat3(instanceMatrix) * n;
	#endif
	vNormalView = normalize(normalMatrix * n);
	vToEye = -mvPosition.xyz;
}
`;

const fragmentShader = /* glsl */ `
#include <common>
#include <clipping_planes_pars_fragment>
uniform vec3 uColor;
uniform float uOpacity;
uniform float uBase;
uniform float uRim;
varying vec3 vNormalView;
varying vec3 vToEye;
void main() {
	#include <clipping_planes_fragment>
	vec3 n = normalize(vNormalView);
	if (!gl_FrontFacing) n = -n;
	vec3 v = normalize(vToEye);
	float rim = pow(1.0 - clamp(abs(dot(n, v)), 0.0, 1.0), 2.0);
	float light = 0.35 + 0.65 * max(dot(n, normalize(vec3(0.3, 0.6, 0.75))), 0.0);
	vec3 colour = uColor * (uBase * light + uRim * rim);
	gl_FragColor = vec4(colour, uOpacity * (0.45 + 0.55 * rim));
	#include <colorspace_fragment>
}
`;

export interface HoloOptions {
	/** 0..1; 1 draws solid (and writes depth). */
	opacity: number;
	/** Face-on brightness. */
	base?: number;
	/** Extra brightness at the rim. */
	rim?: number;
	planes: THREE.Plane[];
}

/** A hologram material in `colour`. */
export function holoMaterial(colour: THREE.ColorRepresentation, o: HoloOptions): THREE.ShaderMaterial {
	const solid = o.opacity >= 1;
	return new THREE.ShaderMaterial({
		uniforms: {
			uColor: { value: new THREE.Color(colour) },
			uOpacity: { value: solid ? 2 : o.opacity },
			uBase: { value: o.base ?? 0.55 },
			uRim: { value: o.rim ?? 0.9 }
		},
		vertexShader,
		fragmentShader,
		transparent: !solid,
		depthWrite: solid,
		side: THREE.DoubleSide,
		clipping: true,
		clippingPlanes: o.planes,
		toneMapped: false
	});
}

/** Set a hologram material's opacity (see {@link holoMaterial}). */
export function setHoloOpacity(m: THREE.ShaderMaterial, opacity: number) {
	m.uniforms.uOpacity.value = opacity;
}
