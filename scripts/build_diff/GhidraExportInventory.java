// Ghidra-side export script (runs inside Ghidra via analyzeHeadless -postScript).
// Writes the raw function inventory of the current program as JSON to the path
// given as the first script argument (or $RUSTOLUTION_OUT when no argument).
//
// Copied from the sibling dance-dance-rustolution-world repo
// (tools/GhidraExportInventory.java); the output-path argument is the only
// local adaptation. scripts/build_diff/run_build_diff.sh drives it.
//
// Java rather than Python: Ghidra 12 routes .py scripts through PyGhidra, which
// the stock analyzeHeadless launcher does not start. Java scripts compile
// in-process with no extra dependencies.
//
// @category Phase0

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.List;
import java.util.Set;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressRange;
import ghidra.program.model.address.AddressSetView;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.FlowType;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.RefType;
import ghidra.program.model.symbol.Reference;

public class GhidraExportInventory extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] scriptArgs = getScriptArgs();
        String outPath = scriptArgs.length > 0 ? scriptArgs[0] : System.getenv("RUSTOLUTION_OUT");
        if (outPath == null || outPath.isEmpty()) {
            throw new IllegalStateException("no output path: pass it as the script argument or set RUSTOLUTION_OUT");
        }
        File out = new File(outPath);
        out.getParentFile().mkdirs();

        FunctionManager fm = currentProgram.getFunctionManager();
        int total = fm.getFunctionCount();
        monitor.initialize(total);
        monitor.setMessage("exporting function inventory");

        try (BufferedWriter w = new BufferedWriter(new OutputStreamWriter(
                new FileOutputStream(out), StandardCharsets.UTF_8))) {
            w.write("{\"build\":" + q(currentProgram.getName()));
            w.write(",\"generated\":" + q(new SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss").format(new Date())));
            w.write(",\"image_base\":" + q(hex(currentProgram.getImageBase().getOffset())));
            w.write(",\"function_count\":" + total);
            w.write(",\"functions\":[\n");

            FunctionIterator it = fm.getFunctions(true);
            boolean first = true;
            int n = 0;
            while (it.hasNext()) {
                monitor.checkCancelled();
                Function fn = it.next();
                if (!first) w.write(",\n");
                first = false;
                w.write(record(fn));
                monitor.incrementProgress(1);
                n++;
            }
            w.write("\n]}\n");
            println("[GhidraExportInventory] wrote " + out + " (" + n + " functions)");
        }
    }

    private String record(Function fn) throws Exception {
        boolean thunk = fn.isThunk();
        boolean external = fn.isExternal();
        AddressSetView body = fn.getBody();
        long size = body.getNumAddresses();

        Set<Function> called = fn.getCalledFunctions(monitor);
        Set<Function> calling = fn.getCallingFunctions(monitor);
        List<String> callees = new ArrayList<>();
        int extCalls = 0;
        for (Function c : called) {
            Function target = c.isThunk() ? c.getThunkedFunction(true) : c;
            if (c.isExternal() || (target != null && target.isExternal())) {
                extCalls++;
            }
            callees.add(q(hex(c.getEntryPoint().getOffset())));
        }

        // Instruction-level signals for the purity heuristic:
        //   indirect_calls — computed CALLs (vtable / function-pointer dispatch)
        //   global_writes / global_reads_rw / global_reads_ro — data refs to
        //     non-stack memory, split by the target block's write permission
        //     (reads of .rdata constants don't break purity; .data does).
        int indirectCalls = 0, globalWrites = 0, globalReadsRw = 0, globalReadsRo = 0;
        Listing listing = currentProgram.getListing();
        Memory mem = currentProgram.getMemory();
        InstructionIterator ii = listing.getInstructions(body, true);
        while (ii.hasNext()) {
            Instruction ins = ii.next();
            FlowType ft = ins.getFlowType();
            if (ft.isCall() && ft.isComputed()) indirectCalls++;
            for (Reference r : ins.getReferencesFrom()) {
                RefType rt = r.getReferenceType();
                if (!rt.isData()) continue;
                Address to = r.getToAddress();
                if (to == null || !to.isMemoryAddress()) continue;
                MemoryBlock blk = mem.getBlock(to);
                if (blk == null) continue;
                if (rt.isWrite()) globalWrites++;
                if (rt.isRead() || !rt.isWrite()) {
                    if (blk.isWrite()) globalReadsRw++; else globalReadsRo++;
                }
            }
        }

        StringBuilder ranges = new StringBuilder();
        for (AddressRange ar : body) {
            if (ranges.length() > 0) ranges.append(',');
            ranges.append('[').append(q(hex(ar.getMinAddress().getOffset())))
                  .append(',').append(q(hex(ar.getMaxAddress().getOffset()))).append(']');
        }

        Namespace ns = fn.getParentNamespace();
        String nsShort = ns == null || ns.isGlobal() ? "" : ns.getName();
        String nsFull = ns == null || ns.isGlobal() ? "" : ns.getName(true);

        StringBuilder sb = new StringBuilder(256);
        sb.append("{\"address\":").append(q(hex(fn.getEntryPoint().getOffset())));
        sb.append(",\"name\":").append(q(fn.getName()));
        sb.append(",\"namespace\":").append(q(nsShort));
        sb.append(",\"namespace_full\":").append(q(nsFull));
        sb.append(",\"source\":").append(q(fn.getSymbol().getSource().toString()));
        sb.append(",\"size\":").append(size);
        sb.append(",\"body\":[").append(ranges).append("]");
        sb.append(",\"thunk\":").append(thunk);
        sb.append(",\"external\":").append(external);
        sb.append(",\"calling_convention\":").append(q(fn.getCallingConventionName()));
        sb.append(",\"param_count\":").append(fn.getParameterCount());
        sb.append(",\"out_calls\":").append(called.size());
        sb.append(",\"external_calls\":").append(extCalls);
        sb.append(",\"indirect_calls\":").append(indirectCalls);
        sb.append(",\"global_writes\":").append(globalWrites);
        sb.append(",\"global_reads_rw\":").append(globalReadsRw);
        sb.append(",\"global_reads_ro\":").append(globalReadsRo);
        sb.append(",\"in_calls\":").append(calling.size());
        sb.append(",\"callees\":[").append(String.join(",", callees)).append("]");
        sb.append(",\"signature\":").append(q(thunk ? "" : fn.getSignature(true).getPrototypeString(true)));
        sb.append("}");
        return sb.toString();
    }

    private static String hex(long v) {
        return "0x" + Long.toHexString(v);
    }

    private static String q(String s) {
        if (s == null) return "null";
        StringBuilder sb = new StringBuilder(s.length() + 2).append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
            }
        }
        return sb.append('"').toString();
    }
}
