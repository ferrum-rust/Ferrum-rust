import java.util.*;
import com.google.gson.Gson;
/** Traversal of the actual Java runtime collection used by MapGenStructure. */
public class HashMapOracle {
    static Map<String,Object> run(String kind,List<String> input) {
        HashMap<Object,Integer> map=new HashMap<Object,Integer>();List<Object> orders=new ArrayList<Object>();
        for(int i=0;i<input.size();i++){Object key=kind.equals("long")?Long.valueOf(input.get(i)):input.get(i);map.put(key,i);List<String> order=new ArrayList<String>();for(Object k:map.keySet())order.add(k.toString());orders.add(order);}
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("kind",kind);result.put("input",input);result.put("orders",orders);return result;
    }
    static long chunk(int x,int z){return (x&0xffffffffL)|((z&0xffffffffL)<<32);}
    public static void main(String[] args) {
        List<Object> cases=new ArrayList<Object>();
        for(int seed=0;seed<6;seed++) {
            Random random=new Random(seed);List<String> keys=new ArrayList<String>();
            for(int i=-80;i<=80;i++)keys.add(Long.toString(chunk(i,i)));Collections.shuffle(keys,random);cases.add(run("long",keys));
            keys=new ArrayList<String>();for(int i=0;i<200;i++){int x=random.nextInt(400)-200,z=random.nextInt(400)-200;keys.add(Long.toString(chunk(x,z)));if(i%17==0)keys.add(keys.get(0));}cases.add(run("long",keys));
            keys=new ArrayList<String>();for(int i=0;i<120;i++)keys.add(Long.toString(chunk(i,i^(i%3==0?64:i%3==1?128:0))));Collections.shuffle(keys,random);cases.add(run("long",keys));
            keys=new ArrayList<String>();for(int i=0;i<128;i++){String s="";for(int bit=0;bit<7;bit++)s+=((i>>bit)&1)==0?"Aa":"BB";keys.add(s);}Collections.shuffle(keys,random);cases.add(run("string",keys));
            keys=new ArrayList<String>();for(int i=0;i<200;i++)keys.add("["+(random.nextInt(400)-200)+","+(random.nextInt(400)-200)+"]");cases.add(run("string",keys));
        }
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("runtime",System.getProperty("java.version"));out.put("cases",cases);System.out.println(new Gson().toJson(out));
    }
}
