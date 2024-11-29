use ark_bn254::Fr;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::{
    fields::fp::FpVar,
    alloc::AllocVar,
    eq::EqGadget,
};
use super::poseidon_hash::PoseidonHasher;
use std::marker::PhantomData;
#[allow(unused_imports)]
use ark_std::rand::{rngs::StdRng, SeedableRng};
#[allow(unused_imports)]
use crate::irrefutable_audit::AuditEvent;

// here we have a complete circuit that proves:
//  - Hash commitment matches event data
//  - Timestamp is valid
//  - Event exists in the Merkle tree

#[derive(Clone)]
pub struct EventCommitmentCircuit {
    // Public inputs (known to witness/verifier)
    pub event_hash: Vec<u8>,
    pub timestamp: i64,
    pub window_id: String,
    pub merkle_root: Vec<u8>,
    pub window_start: i64,
    pub window_end: i64,
    
    // Private inputs (known only to prover)
    pub prover_data: Vec<u8>,
    pub prover_merkle_path: Vec<(Vec<u8>, bool)>,
    
    // System parameters (standardized between prover and verifier)
    pub hasher: PoseidonHasher,
    
    // Phantom data to hold the field type
    _phantom: PhantomData<Fr>
}

impl ConstraintSynthesizer<Fr> for EventCommitmentCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // 1. Hash verification (already started)
        let event_hash_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.event_hash))
        })?;
        
        let prover_data_var = FpVar::<Fr>::new_witness(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.prover_data))
        })?;

        // Verify hash matches
        let computed_hash = self.hasher.hash_leaf_gadget(cs.clone(), &prover_data_var)?;
        computed_hash.enforce_equal(&event_hash_var)?;

        // 2. Timestamp verification
        let timestamp_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.timestamp as u64))
        })?;

        let window_start_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_start as u64))
        })?;

        let window_end_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(Fr::from(self.window_end as u64))
        })?;

        // Enforce timestamp is within window
        timestamp_var.enforce_cmp(&window_start_var, core::cmp::Ordering::Greater, false)?;
        timestamp_var.enforce_cmp(&window_end_var, core::cmp::Ordering::Less, false)?;

        // 3. Merkle path verification
        let merkle_root_var = FpVar::<Fr>::new_input(cs.clone(), || {
            Ok(self.hasher.bytes_to_field_element(&self.merkle_root))
        })?;

        // Verify the Merkle path from event hash to root
        let computed_root = self.verify_merkle_path(cs.clone(), &event_hash_var, &self.prover_merkle_path)?;
        computed_root.enforce_equal(&merkle_root_var)?;

        Ok(())
    }
}

impl EventCommitmentCircuit {
    fn verify_merkle_path(
        &self,
        cs: ConstraintSystemRef<Fr>,
        leaf_hash: &FpVar<Fr>,
        merkle_path: &Vec<(Vec<u8>, bool)>
    ) -> Result<FpVar<Fr>, SynthesisError> {
        let mut current_hash = leaf_hash.clone();

        // Process each level of the Merkle path
        for (sibling_hash, is_left) in merkle_path {
            let sibling_var = FpVar::<Fr>::new_witness(cs.clone(), || {
                Ok(self.hasher.bytes_to_field_element(sibling_hash))
            })?;

            use ark_r1cs_std::boolean::Boolean;
            let _is_left_var = Boolean::new_witness(cs.clone(), || Ok(*is_left))?;

            // If is_left is true:
            //   hash(sibling || current)
            // If is_left is false:
            //   hash(current || sibling)
            let (left, right) = if *is_left {
                (&sibling_var, &current_hash)
            } else {
                (&current_hash, &sibling_var)
            };

            // Use our PoseidonHasher's hash_nodes_gadget
            current_hash = self.hasher.hash_nodes_gadget(cs.clone(), left, right)?;
        }

        Ok(current_hash)
    }
}

#[derive(Clone)]
#[allow(dead_code)] //used in a test
struct SimpleCircuit {
    // Public input
    pub number: u64,
    _phantom: PhantomData<Fr>,
}

impl ConstraintSynthesizer<Fr> for SimpleCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Create variable for our public input
        let a = FpVar::<Fr>::new_input(cs.clone(), || Ok(Fr::from(self.number)))?;
        
        // Create constant 1
        let one = FpVar::<Fr>::new_constant(cs.clone(), Fr::from(1u64))?;
        
        // Assert that a equals 1
        a.enforce_equal(&one)?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Bn254;
    use ark_groth16::Groth16;
    use ark_snark::SNARK;

    #[tokio::test]
    async fn test_witness_verification_flow() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = StdRng::seed_from_u64(0u64);

        // 1. Setup - Create an audit event (simulating prover's action)
        let event = AuditEvent {
            creation_time: "2023-10-01T12:00:00Z".to_string(),
            event_type: "test_event".to_string(),
            file_path: "/test/path1".to_string(),
            event_key: "Martha".to_string(),
        };

        println!("1. Created test event: {:?}", event);

        // 2. Prover creates commitment
        let hasher = PoseidonHasher::new()?;
        let event_bytes = serde_json::to_vec(&event)?;
        let event_hash = hasher.hash_leaf(&event_bytes);
        
        println!("2. Created event hash: {:?}", event_hash);
        
        // Create a valid Merkle path
        let sibling1 = hasher.hash_leaf(b"sibling1");
        let sibling2 = hasher.hash_leaf(b"sibling2");
        
        println!("3. Created sibling hashes:");
        println!("   sibling1: {:?}", sibling1);
        println!("   sibling2: {:?}", sibling2);
        
        // Calculate the actual root by hashing up the path
        let intermediate = hasher.hash_nodes(&event_hash, &sibling1);  // event_hash on left
        println!("4. Intermediate hash: {:?}", intermediate);
        
        let merkle_root = hasher.hash_nodes(&intermediate, &sibling2);  // intermediate on left
        println!("5. Merkle root: {:?}", merkle_root);
        
        let merkle_path = vec![
            (sibling1.clone(), true),   // true means sibling1 goes on left
            (sibling2.clone(), true),   // true means sibling2 goes on left
        ];
        
        println!("6. Created merkle path with directions: {:?}", 
            merkle_path.iter().map(|(_, is_left)| is_left).collect::<Vec<_>>());

        // Create circuit and print public inputs
        let timestamp = 1696161600u64;
        let window_start = 1696118400u64;
        let window_end = 1696204799u64;

        println!("7. Timestamps:");
        println!("   timestamp: {}", timestamp);
        println!("   window_start: {}", window_start);
        println!("   window_end: {}", window_end);

        let circuit = EventCommitmentCircuit {
            event_hash: event_hash.clone(),
            timestamp: timestamp as i64,
            window_id: "2023-10-01".to_string(),
            merkle_root: merkle_root.clone(),
            window_start: window_start as i64,
            window_end: window_end as i64,
            prover_data: event_bytes,
            prover_merkle_path: merkle_path,
            hasher: hasher.clone(),
            _phantom: PhantomData,
        };

        println!("8. Setting up circuit proof system...");
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), &mut rng)?;
        println!("9. Generated proving and verifying keys");
        
        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng)?;
        println!("10. Generated proof");

        let public_inputs = vec![
            hasher.bytes_to_field_element(&event_hash),
            Fr::from(1696161600u64),  // timestamp
            hasher.bytes_to_field_element(&merkle_root),
            Fr::from(1696118400u64),  // window_start
            Fr::from(1696204799u64),  // window_end
        ];

        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs, &proof)?;
        assert!(is_valid, "Proof verification failed!");

        Ok(())
    }
    #[tokio::test]
    async fn test_simple_circuit() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = StdRng::seed_from_u64(0u64);
        
        println!("1. Creating simple circuit...");
        let circuit = SimpleCircuit {
            number: 1u64,
            _phantom: PhantomData,
        };
        
        println!("2. Setting up circuit proof system...");
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit.clone(), &mut rng)?;
        
        println!("3. Generating proof...");
        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng)?;
        
        println!("4. Verifying proof...");
        let public_inputs = vec![Fr::from(1u64)];
        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs, &proof)?;
        
        assert!(is_valid, "Proof verification failed!");
        println!("5. Proof verified successfully!");
        
        Ok(())
    }
}