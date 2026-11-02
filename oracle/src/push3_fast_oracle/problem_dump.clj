(ns push3-fast-oracle.problem-dump
  "Differential and speed data on a real Propeller problem: random programs drawn from the
  problem's own alphabet (ERCs included), run on the problem's own training cases, step limit as
  in the benchmark. One EDN line per (program, case).

  Usage: ... problem-dump <problem> <n-programs> <n-cases> <seed> <out.edn>"
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.input-output :as io]
   [benjamin-schwerdtner.clj-push3-play.interpreter :as push]
   [benjamin-schwerdtner.problems.propeller.gp :as gp]
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [push3-fast-oracle.dump :as dump]
   [clojure.java.io :as io*]
   [random-seed.core :as rs]))

(def ^:private max-nodes
  "Cases whose final EXEC stack is larger than this are skipped: exec_y/exec_s/exec_k can build
  stacks that take longer to print than to run, and both sides run the same recorded set."
  20000)

(defn- tree-size
  "Number of nodes in x, counting stops at `limit`."
  [x limit]
  (loop [stack (list x) n 0]
    (cond
      (>= n limit) n
      (empty? stack) n
      :else (let [[h & t] stack]
              (recur (if (sequential? h) (concat h t) t) (inc n))))))

(defn- state-of [program inputs input-names max]
  (let [t0 (System/nanoTime)
        st (-> (push/setup-state)
               (assoc :exceptions-are-warnings? true)
               (io/with-inputs (if input-names (zipmap input-names inputs) inputs))
               (push/execute program {:max-executions max}))
        ns (- (System/nanoTime) t0)]
    [ns (dump/final-state st)]))

(defn -main [problem n-programs n-cases seed out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [p (problems/by-name (keyword problem))
          atoms (problems/atom-generators p)
          cases (:train (problems/train-and-test p (parse-long n-cases) 10 0))
          programs (binding [rs/*rng* (rs/random (parse-long seed))]
                     (vec (repeatedly (parse-long n-programs) #(gp/random-genome atoms 100))))
          exec-ns (atom 0) steps (atom 0) written (atom 0) failed (atom 0) skipped (atom 0)]
      (with-open [w (io*/writer out)]
        (doseq [prog programs, c cases]
          (try
(let [[ns st] (state-of prog (:inputs c) (:input-names p) run/default-step-limit)]
              (if (>= (tree-size (:exec st) max-nodes) max-nodes)
                (swap! skipped inc)
                (do (swap! exec-ns + ns) (swap! steps + (:steps st)) (swap! written inc)
                    (.write w (str (pr-str (merge {:program prog :inputs (:inputs c) :max run/default-step-limit}
                                                  (when (:input-names p) {:input-names (:input-names p)})
                                                  st))
                                   "\n")))))
            (catch Throwable _ (swap! failed inc)))))
      (let [ex (/ @exec-ns 1e9)]
        (println (format "%s: %d runs written, %d skipped (state too large or unmodelled stack non-empty), %d failed, %d steps; oracle execute only %.2f s = %.0f runs/s, %.3f M steps/s"
                         problem @written @skipped @failed @steps ex (/ @written ex) (/ @steps ex 1e6))))))
  (shutdown-agents))
