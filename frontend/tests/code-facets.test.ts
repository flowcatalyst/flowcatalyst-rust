/**
 * The dispatch jobs page's application, subdomain and aggregate filters take
 * their options from the distinct codes (bug 19 in docs/ui/README.md,
 * docs/platform-bug-fixes.md): they were always empty.
 */

import { describe, expect, it } from "vitest";
import { codeLevels } from "@/utils/code-facets";

const codes = [
	"orders:billing:invoice:created",
	"orders:billing:invoice:paid",
	"orders:shipping:parcel:sent",
	"crm:contacts:person:updated",
];

describe("codeLevels", () => {
	it("derives every level from the codes", () => {
		expect(codeLevels(codes)).toEqual({
			applications: ["crm", "orders"],
			subdomains: ["billing", "contacts", "shipping"],
			aggregates: ["invoice", "parcel", "person"],
			codes: [...codes].sort(),
		});
	});

	it("narrows each level by the values chosen above it", () => {
		const l = codeLevels(codes, ["orders"], ["billing"]);
		expect(l.applications).toEqual(["crm", "orders"]);
		expect(l.subdomains).toEqual(["billing", "shipping"]);
		expect(l.aggregates).toEqual(["invoice"]);
		expect(l.codes).toEqual([
			"orders:billing:invoice:created",
			"orders:billing:invoice:paid",
		]);
	});
});
