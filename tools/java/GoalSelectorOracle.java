import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.ai.*;
import net.minecraft.profiler.Profiler;

/** EntityAITasks callback order, priorities, removals and restarts. */
public final class GoalSelectorOracle {
    static final class Owner {
        int ticks;
        final List<Object> calls=new ArrayList<>();
    }
    static final class Goal extends EntityAIBase {
        final Owner owner;
        final int id;
        final int seed;
        Goal(Owner owner,int id,int mask,int seed){this.owner=owner;this.id=id;this.seed=seed;setMutexBits(mask);}
        void call(String name){owner.calls.add(Arrays.asList(name,id));}
        public boolean shouldExecute(){call("test");return Math.floorMod(owner.ticks*7+seed+id*11,13)<7;}
        public boolean continueExecuting(){call("continue");return Math.floorMod(owner.ticks*3+seed+id*17,19)<12;}
        public boolean isInterruptible(){return (id+seed)%4!=0;}
        public void startExecuting(){call("start");}
        public void resetTask(){call("reset");}
        public void updateTask(){call("update");}
    }
    static Object run(int n){
        Random random=new Random(-189L+n*113);
        Owner owner=new Owner();EntityAITasks tasks=new EntityAITasks(new Profiler());
        List<Object> defs=new ArrayList<>(),registrations=new ArrayList<>(),frames=new ArrayList<>();
        List<Goal> goals=new ArrayList<>();
        int count=3+n%8;
        for(int i=0;i<count;i++){int mask=random.nextInt(8),seed=random.nextInt(70);goals.add(new Goal(owner,i,mask,seed));defs.add(Arrays.asList(mask,seed));}
        for(int i=0;i<count+2;i++){int id=random.nextInt(count),priority=random.nextInt(6);tasks.addTask(priority,goals.get(id));registrations.add(Arrays.asList(id,priority));}
        for(int t=1;t<=72;t++){
            owner.ticks=t;owner.calls.clear();List<Object> edits=new ArrayList<>();
            if(t==9||t==36||t==50){int id=random.nextInt(count);tasks.removeTask(goals.get(id));edits.add(Arrays.asList("remove",id));}
            if(t==15||t==36||t==54){int id=random.nextInt(count),priority=random.nextInt(6);tasks.addTask(priority,goals.get(id));edits.add(Arrays.asList("add",id,priority));}
            tasks.onUpdateTasks();frames.add(Arrays.asList(edits,new ArrayList<Object>(owner.calls)));
        }
        return Arrays.asList(defs,registrations,frames);
    }
    public static void main(String[]args)throws Exception{
        List<Object> cases=new ArrayList<>();for(int i=0;i<144;i++)cases.add(run(i));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original EntityAITasks 144 selectors / 10,368 updates");
    }
}
