(ns push3-fast-oracle.plushy-check
  "Random genomes, translated by Propeller's own `plushy->push`; the genomes go to genomes.edn and
  the expected programs to programs.edn, for `push3-fast plushy` to reproduce.
  Needs Propeller's sources on the classpath: -Sdeps '{:paths [\"src\" \"<propeller>/src\"]}'.

  Usage: ... plushy-check <n> <seed> <genomes.edn> <programs.edn>"
  (:require
   [clojure.string :as str]
   [propeller.genome :as genome])
  (:import (java.util Random)))

;; the instructions with a block in Propeller, under names both interpreters share, plus plain ones
(def pool
  [:exec_dup :exec_dup_times :exec_pop :exec_shove :exec_swap :exec_if :exec_k :exec_s :exec_rot
   :exec_when :exec_while :exec_do_while :exec_y :string_iterate
   :integer_+ :integer_dup :in1 1 2 "s" true 'close 'close 'close])

(defn -main [n seed genomes programs]
  (let [rng (Random. (parse-long (str seed)))
        gen #(vec (repeatedly (.nextInt rng 40) (fn [] (nth pool (.nextInt rng (count pool))))))
        gs (vec (repeatedly (parse-long (str n)) gen))
        ->kw (fn [x] (if (= x 'close) :close x))]
    (spit genomes (str/join "\n" (map #(pr-str (mapv ->kw %)) gs)))
    ;; Propeller writes the program with symbols and keywords as they are; `close` never appears in it
    (spit programs (str/join "\n" (map #(pr-str (genome/plushy->push %)) gs))))
  (shutdown-agents))
