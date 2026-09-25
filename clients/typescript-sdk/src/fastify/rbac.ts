/**
 * RBAC catalogue — declarative role → permission map evaluated locally
 * inside the app process.
 *
 * Why local: FlowCatalyst tokens carry **roles only**. Permissions belong
 * to the consumer app (what an "invoice:void" capability even means is
 * app-specific). Defining them in code keeps them version-controlled,
 * testable, and avoids round-tripping to the platform on every guard.
 *
 *     const rbac = defineRbac()
 *       .role("billing-admin").grants("invoice:create", "invoice:read")
 *       .role("billing-viewer").grants("invoice:read")
 *       .role("support").grants("ticket:*")
 *       .build();
 *
 * Wildcards are suffix-only on `:` boundaries:
 *   "ticket:*"  matches  "ticket:read", "ticket:close"
 *   "*"         matches  everything
 *   "ticket:r*" does NOT match anything — mid-segment globs are not supported.
 *
 * Resolution: union of all permissions across the principal's roles.
 * Unknown roles are silently ignored (the catalogue is the source of truth
 * for what an app cares about; foreign roles from other apps just don't
 * grant anything here).
 *
 * ## Qualified vs bare role names
 *
 * FlowCatalyst names a role canonically as `"{applicationCode}:{role}"`, but
 * the `roles` claim has historically reached apps in either form: qualified
 * from most mint paths, bare from a platform build that stripped the prefix
 * for application-scoped OAuth clients. An app should not have to care which
 * one it got, or declare its roles twice to cover both.
 *
 * So declare each role in whichever form reads best — usually the bare,
 * app-local one — and the catalogue matches the other form too:
 *
 *     defineRbac().role("bidder").grants("rfp:bid:submit")
 *     // resolves a claim of "bidder" AND of "rfp:bidder"
 *
 * A catalogue describes one application, so a bare name is unambiguous inside
 * it. The one exception is a catalogue that declares two roles collapsing to
 * the same bare name (`"admin"` and `"hr:admin"`): those are left to exact
 * matching only, since guessing which was meant could over-grant.
 */

export interface RbacCatalogue {
	readonly roles: ReadonlyMap<string, ReadonlySet<string>>;
	resolve(roleNames: readonly string[]): string[];
}

class RoleScope {
	private readonly builder: RbacBuilder;
	private readonly roleName: string;

	constructor(builder: RbacBuilder, roleName: string) {
		this.builder = builder;
		this.roleName = roleName;
	}

	grants(...permissions: string[]): RbacBuilder {
		this.builder._grant(this.roleName, permissions);
		return this.builder;
	}
}

export class RbacBuilder {
	private readonly map = new Map<string, Set<string>>();

	role(name: string): RoleScope {
		if (!name) throw new Error("RBAC role name cannot be empty");
		if (!this.map.has(name)) this.map.set(name, new Set());
		return new RoleScope(this, name);
	}

	/** @internal */
	_grant(role: string, permissions: string[]): void {
		const bucket = this.map.get(role);
		if (!bucket) throw new Error(`RBAC role "${role}" not declared`);
		for (const p of permissions) {
			if (!p) throw new Error(`RBAC permission for role "${role}" is empty`);
			bucket.add(p);
		}
	}

	build(): RbacCatalogue {
		const frozen: ReadonlyMap<string, ReadonlySet<string>> = new Map(
			Array.from(this.map.entries()).map(([k, v]) => [k, new Set(v)]),
		);

		// Secondary index for the bare-claim/qualified-declaration direction:
		// a role declared as "hr:hr-manager" also answers to "hr-manager". An
		// exact declaration of the bare name wins over it, and a bare name two
		// declarations would both claim is left out entirely (null) rather than
		// granting the union of both.
		const byBareName = new Map<string, ReadonlySet<string> | null>();
		for (const [name, perms] of frozen) {
			const bare = bareRoleName(name);
			if (bare === name || frozen.has(bare)) continue;
			byBareName.set(bare, byBareName.has(bare) ? null : perms);
		}

		return {
			roles: frozen,
			resolve(roleNames) {
				const out = new Set<string>();
				for (const role of roleNames) {
					// Exact, then qualified-claim/bare-declaration (strip the
					// claim's application prefix), then the reverse direction.
					const perms =
						frozen.get(role) ??
						frozen.get(bareRoleName(role)) ??
						byBareName.get(role);
					if (!perms) continue;
					for (const p of perms) out.add(p);
				}
				return [...out];
			},
		};
	}
}

export function defineRbac(): RbacBuilder {
	return new RbacBuilder();
}

/**
 * The app-local part of a role name: everything after the first `:`.
 * `"hr:hr-manager"` → `"hr-manager"`, `"bidder"` → `"bidder"`.
 *
 * Only the FIRST colon delimits, so a legacy multi-segment role
 * (`"logistics_portal:dashboard:user"`) keeps its inner colon
 * (`"dashboard:user"`) rather than being truncated.
 */
function bareRoleName(name: string): string {
	const i = name.indexOf(":");
	return i === -1 ? name : name.slice(i + 1);
}
