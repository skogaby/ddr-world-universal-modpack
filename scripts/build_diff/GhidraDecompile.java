// Headless decompile helper: writes decompiler C + disassembly for a list of
// functions to a scratch directory OUTSIDE the repo (decompiler output is
// Konami code and must never be committed).
//
//   RUSTOLUTION_RVAS="0x611d0,0x24970"  RUSTOLUTION_OUT_DIR=<scratch dir>
//   analyzeHeadless <dir> <project> -process <program> -noanalysis \
//       -scriptPath scripts/build_diff -postScript GhidraDecompile.java
//
// Copied verbatim from the sibling dance-dance-rustolution-world repo
// (tools/GhidraDecompile.java). scripts/build_diff/asm_diff.py drives it.
//
// @category Phase0

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;

public class GhidraDecompile extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String rvas = System.getenv("RUSTOLUTION_RVAS");
        String outDir = System.getenv("RUSTOLUTION_OUT_DIR");
        if (rvas == null || outDir == null) {
            throw new IllegalStateException("RUSTOLUTION_RVAS and RUSTOLUTION_OUT_DIR must be set");
        }
        File dir = new File(outDir);
        dir.mkdirs();
        String module = currentProgram.getName().replaceAll("_\\d{8}.*$", "").replaceAll("\\.dll$", "");

        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        long base = currentProgram.getImageBase().getOffset();
        for (String tok : rvas.split(",")) {
            tok = tok.trim();
            if (tok.isEmpty()) continue;
            long rva = Long.decode(tok);
            Address a = toAddr(base + rva);
            Function fn = getFunctionContaining(a);
            if (fn == null) {
                println("no function at rva " + tok);
                continue;
            }
            File out = new File(dir, module + "_" + tok + ".txt");
            try (PrintWriter w = new PrintWriter(out, StandardCharsets.UTF_8)) {
                w.println("// " + fn.getName() + " @ " + fn.getEntryPoint() + "  rva " + tok
                        + "  size " + fn.getBody().getNumAddresses());
                w.println("// signature: " + fn.getSignature(true).getPrototypeString(true));
                String sha = currentProgram.getExecutableSHA256();
                w.println("// program: " + currentProgram.getName() + "  sha256 " + (sha == null ? "unknown" : sha));
                DecompileResults res = ifc.decompileFunction(fn, 60, monitor);
                if (res.decompileCompleted()) {
                    String c = res.getDecompiledFunction().getC();
                    // Ghidra replaces some CRT calls with pseudo-ops and says so in a header
                    // warning (`__security_check_cookie replaced with injection`): the call
                    // and its effect on RAX vanish from the C. Say so where the lane reads.
                    if (c.contains("replaced with injection")) {
                        w.println("// NOTE: a call below was replaced by a Ghidra injection and is NOT in the C text");
                        w.println("//       (the WARNING names it). Its position and its effect on RAX/XMM0 are in the");
                        w.println("//       disassembly only (lane-brief §3): the return value is what is in RAX at RET.");
                    }
                    w.println(c);
                } else {
                    w.println("// decompile failed: " + res.getErrorMessage());
                }
                w.println("// ---- disassembly ----");
                InstructionIterator it = currentProgram.getListing().getInstructions(fn.getBody(), true);
                while (it.hasNext()) {
                    Instruction ins = it.next();
                    w.println(String.format("%x  %s", ins.getAddress().getOffset() - base, ins));
                }
            }
            println("wrote " + out);
        }
        ifc.dispose();
    }
}
