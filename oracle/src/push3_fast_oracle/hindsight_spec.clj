(ns push3-fast-oracle.hindsight-spec
  "Writes the spec file `push3-fast hindsight` reads, for one Propeller problem: its instruction
  alphabet (the ERC generator `random-int` becomes `:int-erc [-100 100]`, each other generator one
  pool of drawn values in `:erc-pools`), its output kind, a pool of input tuples from the problem's
  own input generator, and the step limit, genome size and `close` gene of its Propeller file.

  Usage: ... hindsight-spec <problem> <pool-size> <seed> <out.edn>"
  (:require
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [random-seed.core :as rs]))

(defn- propeller-settings
  "What the problem's Propeller file sets: `:step-limit`, `:max-initial-plushy-size`, and whether its
  instruction list has a `close` gene. `:source` is relative to the clj-push3 checkout."
  [{:keys [source]}]
  (let [text (slurp (str "../../benjamin-schwerdtner-clj-push3/" source))
        number (fn [k] (some-> (re-find (re-pattern (str k "\\s+(\\d+)")) text) second parse-long))]
    {:step-limit (number ":step-limit")
     :max-size (number ":max-initial-plushy-size")
     :close? (boolean (re-find #"'close" text))}))

(defn spec [p pool-size seed]
  (let [{:keys [step-limit max-size close?]} (propeller-settings p)
        alphabet (problems/alphabet p)
        generator? #(and (fn? %) (not= % problems/random-int))
        ;; one slot per generator, as in Propeller, which calls the function for each gene
        pools (binding [rs/*rng* (rs/random seed)]
                (mapv (fn [f] (vec (repeatedly 1000 f))) (filter generator? alphabet)))
        pool (->> (:train (problems/train-and-test p pool-size 10 seed)) (mapv :inputs))]
    (merge {:alphabet (cond-> (vec (remove fn? alphabet)) close? (conj :close))
            :erc-pools pools
            :output (:output p)
            :max (or step-limit run/default-step-limit)
            :inputs pool}
           (when max-size {:max-initial-plushy-size max-size})
           (when (some #(= % problems/random-int) alphabet) {:int-erc [-100 100]})
           (select-keys p [:num-outputs :input-names]))))

(defn -main [problem pool-size seed out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (spit out (pr-str (spec (problems/by-name (keyword problem)) (parse-long pool-size) (parse-long seed))))
    (println problem "->" out))
  (shutdown-agents))
