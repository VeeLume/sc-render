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
	#ifdef GHOST
		gl_FragColor = vec4(colour * uOpacity, 1.0);
	#else
		gl_FragColor = vec4(colour, uOpacity * (0.45 + 0.55 * rim));
	#endif
	#include <colorspace_fragment>
	#ifdef GHOST
		// Alpha as strong as the colour, so a transparent canvas composites it right.
		gl_FragColor.a = max(gl_FragColor.r, max(gl_FragColor.g, gl_FragColor.b));
	#endif
}
`;

export interface HoloOptions {
	/** 0..1; 1 draws solid (and writes depth). */
	opacity: number;
	/** Face-on brightness. */
	base?: number;
	/** Extra brightness at the rim. */
	rim?: number;
	/**
	 * A solid surface drawn after the see-through hull, so the hull's layers
	 * do not paint over it (give the mesh a higher `renderOrder`).
	 */
	overHull?: boolean;
	/**
	 * See-through by keeping the brightest surface at each pixel instead of
	 * blending them (see {@link MAX_BLEND}); `opacity` is then its strength.
	 */
	ghost?: boolean;
	planes: THREE.Plane[];
}

/**
 * Keep the brightest of what lands on a pixel. However many of a ship's
 * layers stack up, the result is no brighter than one of them: alpha blending
 * builds them up to a solid colour, adding them burns to white.
 */
export const MAX_BLEND = {
	blending: THREE.CustomBlending,
	blendEquation: THREE.MaxEquation,
	blendSrc: THREE.OneFactor,
	blendDst: THREE.OneFactor
} as const;

/** A hologram material in `colour`. */
export function holoMaterial(colour: THREE.ColorRepresentation, o: HoloOptions): THREE.ShaderMaterial {
	const solid = o.opacity >= 1;
	return new THREE.ShaderMaterial({
		...(o.ghost ? { ...MAX_BLEND, defines: { GHOST: 1 } } : {}),
		uniforms: {
			uColor: { value: new THREE.Color(colour) },
			// Solid: enough that alpha is 1 even face-on (0.45 x 3).
			uOpacity: { value: solid ? 3 : o.opacity },
			uBase: { value: o.base ?? 0.55 },
			uRim: { value: o.rim ?? 0.9 }
		},
		vertexShader,
		fragmentShader,
		// In three's transparent queue, the only way to come after the hull.
		transparent: !solid || !!o.overHull,
		depthWrite: solid,
		// Over the hull means over it: the depth pre-pass holds the hull's skin.
		depthTest: !o.overHull,
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
