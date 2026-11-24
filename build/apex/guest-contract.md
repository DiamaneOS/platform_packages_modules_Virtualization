# Protected guest contract

`guest_vm_contract.py` is the shared definition for archive structure, role metadata,
full-length SHA-256 descriptors and AVB signatures. diamaneos-tools vendors this file
byte for byte as `src/diamaneos_tools/guest_vm_contract.py`. Change both copies together
and compare their bytes before landing. The tools tests pin the shared file's digest.

The release signer carries `guest_vm_role_digests.py` inside its Python executable.
Soong generates it directly from `decoder_vm_initrd` and `fsvm_guest_initrd`, independently
of the directory subsequently supplied to the signer. A replacement pair cannot replace
that embedded identity. Use the signing executable from the trusted build's otatools.
The APEX carries no input-controlled role manifest and no new release key is involved.

Standalone source verification and diamaneos-tools use the reviewed digests in the
contract module. They identify the initramfs outputs in set 20261120-userdebug-950170b55f.
These are independent of the pair's AVB descriptors and the packaged APEX. The image gate
also retains the independent guest kernel body pin.

An intentional initramfs change needs new producer outputs, review of the changed role,
updated source digests in both copies, and the image mutation tests. A new build signer
embeds the new producer digests automatically. Its hashes alone do not establish that the
producer sources were reviewed or that the guest works on the phone. Builder and device
acceptance remain separate.
