# Protected guest contract

`guest_vm_contract.py` is the shared definition for archive structure, role metadata,
full-length SHA-256 descriptors and AVB signatures. diamaneos-tools vendors this file
byte for byte as `src/diamaneos_tools/guest_vm_contract.py`. Change both copies together
and compare their bytes before landing. The tools test (test_guest_contract) and
sign_virt_apex_guest_test.py pin the same digest of the shared file, so a change to one copy
alone fails a test.

The release signer carries `guest_vm_role_digests.py` inside its Python executable.
Soong generates it directly from `decoder_vm_initrd` and `fsvm_guest_initrd`, independently
of the directory subsequently supplied to the signer. A replacement pair cannot replace
that embedded identity. Use the signing executable from the trusted build's otatools.
The APEX carries no input-controlled role manifest and no new release key is involved.

Standalone source verification and diamaneos-tools use the reviewed digests in the
contract module. They identify the initramfs outputs of the 2026-11-24 squash sources, built
twice with the same bytes (user set 20261125-user-c37ab2cfdd and a separate build).
These are independent of the pair's AVB descriptors and the packaged APEX. The image gate
also retains the independent guest kernel body pin.

An intentional initramfs change needs new producer outputs, review of the changed role,
updated source digests in both copies, and the image mutation tests. A new build signer
embeds the new producer digests automatically. Its hashes alone do not establish that the
producer sources were reviewed or that the guest works on the phone. Builder and device
acceptance remain separate.
