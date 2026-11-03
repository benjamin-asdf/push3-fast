(ns push3-fast-oracle.hindsight-spec
  "Writes the spec file `push3-fast hindsight` reads, for one Propeller problem: its instruction
  alphabet (the ERC generator `random-int` becomes `:int-erc [-100 100]`, other ERC generators are
  sampled into literals), its output kind, and a pool of input tuples from the problem's own input
  generator.

  Usage: ... hindsight-spec <problem> <pool-size> <seed> <out.edn>"
  (:require
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [random-seed.core :as rs]))

(defn spec [p pool-size seed]
  (let [alphabet (problems/alphabet p)
        literal? (complement fn?)
        samples (binding [rs/*rng* (rs/random seed)]
                  (into [] (comp (filter #(and (fn? %) (not= % problems/random-int)))
                                 (mapcat #(repeatedly 30 %)))
                        alphabet))
        pool (->> (:train (problems/train-and-test p pool-size 10 seed)) (mapv :inputs))]
    (merge {:alphabet (into (filterv literal? alphabet) samples)
            :output (:output p)
            :max run/default-step-limit
            :inputs pool}
           (when (some #(= % problems/random-int) alphabet) {:int-erc [-100 100]})
           (select-keys p [:num-outputs :input-names]))))

(defn -main [problem pool-size seed out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (spit out (pr-str (spec (problems/by-name (keyword problem)) (parse-long pool-size) (parse-long seed))))
    (println problem "->" out))
  (shutdown-agents))
