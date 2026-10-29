// Static PE analysis only: the vendor process is never executed.
// @category AJAZZ
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class ExportKeyboardUpdater extends GhidraScript {
    @Override public void run() throws Exception {
        File out = new File(getScriptArgs()[0]); out.mkdirs();
        Set<Function> roots = new TreeSet<>(Comparator.comparing(f -> f.getEntryPoint()));
        try (PrintWriter refs = new PrintWriter(new File(out,"references.tsv"),StandardCharsets.UTF_8)) {
            refs.println("kind\ttarget\tfrom\tfunction");
            String[] needles = {"SN34F280", "Enable Program", "Get Firmware Version", "Check ISP Password", "Check CheckSum", "Get User Code CheckSum", "ProgramByDeviceVersionInfo", "CheckBootLoaderID", "SpecialPID", "CheckDeviceCmd", "Security = CS3", "Device security level", "RETURN_ACK_FAILED", "12345678"};
            for(String needle : needles) for(String enc : new String[]{"US-ASCII","UTF-16LE"}) {
                Address at = currentProgram.getImageBase();
                byte[] bytes=needle.getBytes(enc);
                while((at=currentProgram.getMemory().findBytes(at,bytes,null,true,monitor))!=null) {
                    for(Reference ref : getReferencesTo(at)) {
                        Function f=getFunctionContaining(ref.getFromAddress());
                        refs.println(needle+"\t"+at+"\t"+ref.getFromAddress()+"\t"+(f==null?"data":f.getEntryPoint()));
                        if(f!=null) roots.add(f);
                    }
                    at=at.add(bytes.length);
                }
            }
            SymbolIterator symbols=currentProgram.getSymbolTable().getAllSymbols(true);
            while(symbols.hasNext()) {
                Symbol s=symbols.next();
                if(!s.getName().matches(".*(HidD_(SetFeature|GetFeature)|FindResourceW|GetPrivateProfile.*|Sleep).*")) continue;
                for(Reference ref : getReferencesTo(s.getAddress())) {
                    Function f=getFunctionContaining(ref.getFromAddress());
                    refs.println(s.getName()+"\t"+s.getAddress()+"\t"+ref.getFromAddress()+"\t"+(f==null?"data":f.getEntryPoint()));
                    if(f!=null && f.getEntryPoint().getOffset()<0x440000)roots.add(f);
                }
            }
        }
        Set<Function> selected = new TreeSet<>(Comparator.comparing(f -> f.getEntryPoint()));
        selected.addAll(roots);
        for(int depth=0;depth<2;depth++) for(Function f:new ArrayList<>(selected)) {
            for(Function callee : f.getCalledFunctions(monitor))
                if(!callee.isExternal() && callee.getEntryPoint().getOffset()<0x440000)selected.add(callee);
            if(depth==0) for(Function caller : f.getCallingFunctions(monitor))
                if(caller.getEntryPoint().getOffset()<0x440000)selected.add(caller);
        }
        DecompInterface d=new DecompInterface();d.openProgram(currentProgram);
        long deadline=System.nanoTime()+240_000_000_000L; int count=0;
        try(PrintWriter code=new PrintWriter(new File(out,"updater.c"),StandardCharsets.UTF_8);
            PrintWriter index=new PrintWriter(new File(out,"functions.tsv"),StandardCharsets.UTF_8)) {
            index.println("entry\tname\tbytes\troot\tdecompiled");
            for(Function f:selected) {
                if(monitor.isCancelled() || System.nanoTime()>deadline)break;
                DecompileResults r=d.decompileFunction(f,6,monitor);
                boolean ok=r.decompileCompleted()&&r.getDecompiledFunction()!=null;
                index.println(f.getEntryPoint()+"\t"+f.getName()+"\t"+f.getBody().getNumAddresses()+"\t"+roots.contains(f)+"\t"+ok);
                if(ok) {
                    code.println("\n/* ENTRY "+f.getEntryPoint()+" */");code.println(r.getDecompiledFunction().getC());
                    try(PrintWriter one=new PrintWriter(new File(out,f.getEntryPoint()+".c"),StandardCharsets.UTF_8)){one.println(r.getDecompiledFunction().getC());}
                    count++;
                }
            }
        } finally {d.dispose();}
        println("Keyboard updater roots="+roots.size()+" selected="+selected.size()+" exported="+count);
    }
}
