use ferriscrypt::asym::ec_key::{generate_keypair, Curve};
use mls::ciphersuite::CipherSuite;
use mls::ciphersuite::CipherSuite::{
    Mls10128Dhkemp256Aes128gcmSha256P256, Mls10128Dhkemx25519Aes128gcmSha256Ed25519,
    Mls10128Dhkemx25519Chacha20poly1305Sha256Ed25519, Mls10256Dhkemp521Aes256gcmSha512P521,
};
use mls::client::Client;
use mls::credential::{BasicCredential, Credential};
use mls::extension::LifetimeExt;
use mls::group::{Event, Group};
use mls::key_package::{KeyPackage, KeyPackageGeneration};
use std::time::SystemTime;

fn generate_client(cipher_suite: CipherSuite, id: Vec<u8>) -> Client {
    let (public_key, secret_key) =
        generate_keypair(Curve::from(cipher_suite.signature_scheme())).unwrap();
    let credential = Credential::Basic(BasicCredential::new(id, public_key).unwrap());
    Client::new(cipher_suite, secret_key, credential).unwrap()
}

fn test_create(cipher_suite: CipherSuite, update_path: bool) {
    let alice = generate_client(cipher_suite, b"alice".to_vec());
    let bob = generate_client(cipher_suite, b"bob".to_vec());
    let key_lifetime = LifetimeExt::years(1, SystemTime::now()).unwrap();

    let alice_key = alice.gen_key_package(&key_lifetime).unwrap();
    let bob_key = bob.gen_key_package(&key_lifetime).unwrap();

    // Alice creates a group and adds bob to the group
    let mut test_group = Group::new(b"group".to_vec(), alice_key).unwrap();

    let add_members = test_group
        .add_member_proposals(&[bob_key.key_package.clone()])
        .unwrap();

    let commit = test_group
        .commit_proposals(add_members, update_path, &alice.signature_key)
        .unwrap();

    // Upon server confirmation, alice applies the commit to her own state
    test_group.process_pending_commit(commit.clone()).unwrap();

    // Bob receives the welcome message and joins the group
    let bob_group = Group::from_welcome_message(
        commit.welcome.unwrap(),
        test_group.public_tree.clone(),
        bob_key,
    )
    .unwrap();

    assert_eq!(test_group, bob_group);
}

fn get_cipher_suites() -> Vec<CipherSuite> {
    [
        Mls10128Dhkemx25519Aes128gcmSha256Ed25519,
        Mls10256Dhkemp521Aes256gcmSha512P521,
        Mls10128Dhkemx25519Chacha20poly1305Sha256Ed25519,
        Mls10128Dhkemp256Aes128gcmSha256P256,
    ]
    .to_vec()
}

#[test]
fn test_create_group_no_update() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_create(cs.clone(), false))
}

#[test]
fn test_create_group_update() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_create(cs.clone(), true))
}

struct TestGroupCreation {
    creator: Client,
    #[allow(dead_code)]
    creator_key: KeyPackageGeneration,
    creator_group: Group,
    receiver_clients: Vec<Client>,
    #[allow(dead_code)]
    receiver_private_keys: Vec<KeyPackageGeneration>,
    receiver_groups: Vec<Group>,
}

fn get_test_group(cipher_suite: CipherSuite, num_participants: usize) -> TestGroupCreation {
    // Create the group with Alice as the group initiator
    let alice = generate_client(cipher_suite, b"alice".to_vec());
    let key_lifetime = LifetimeExt::years(1, SystemTime::now()).unwrap();

    let alice_key = alice.gen_key_package(&key_lifetime).unwrap();

    let mut test_group = Group::new(b"group".to_vec(), alice_key.clone()).unwrap();

    // Generate random clients that will be members of the group
    let clients = (0..num_participants)
        .into_iter()
        .map(|_| generate_client(cipher_suite, b"test".to_vec()))
        .collect::<Vec<Client>>();

    let test_keys = clients
        .iter()
        .map(|client| client.gen_key_package(&key_lifetime).unwrap())
        .collect::<Vec<KeyPackageGeneration>>();

    // Add the generated clients to the group Alice created
    let add_members_proposal = test_group
        .add_member_proposals(
            &test_keys
                .iter()
                .map(|g| g.key_package.clone())
                .collect::<Vec<KeyPackage>>(),
        )
        .unwrap();

    let commit = test_group
        .commit_proposals(add_members_proposal, true, &alice.signature_key)
        .unwrap();

    let events = test_group.process_pending_commit(commit.clone()).unwrap();
    assert_eq!(events.len(), 1);
    let credentials = test_keys
        .iter()
        .map(|k| k.key_package.credential.clone())
        .collect();
    assert_eq!(events[0], Event::MembersAdded(credentials));

    // Create groups for each participant by processing Alice's welcome message
    let receiver_groups = test_keys
        .iter()
        .map(|kp| {
            Group::from_welcome_message(
                commit.welcome.as_ref().unwrap().clone(),
                test_group.public_tree.clone(),
                kp.clone(),
            )
            .unwrap()
        })
        .collect::<Vec<Group>>();

    TestGroupCreation {
        creator: alice,
        creator_key: alice_key,
        creator_group: test_group,
        receiver_clients: clients,
        receiver_private_keys: test_keys,
        receiver_groups,
    }
}

fn test_path_updates(cipher_suite: CipherSuite) {
    println!("Testing path updates for cipher suite: {:?}", cipher_suite);

    let mut test_group_data = get_test_group(cipher_suite, 10);

    // Loop through each participant and send a path update
    for i in 0..test_group_data.receiver_groups.len() {
        let pending = test_group_data.receiver_groups[i]
            .commit_proposals(
                vec![],
                true,
                &test_group_data.receiver_clients[i].signature_key,
            )
            .unwrap();

        test_group_data
            .creator_group
            .process_plaintext(pending.plaintext.clone())
            .unwrap();

        for j in 0..test_group_data.receiver_groups.len() {
            if i != j {
                test_group_data.receiver_groups[j]
                    .process_plaintext(pending.plaintext.clone())
                    .unwrap();
            } else {
                test_group_data.receiver_groups[j]
                    .process_pending_commit(pending.clone())
                    .unwrap();
            }
        }
    }

    // Validate that all the groups are in the same end state
    test_group_data
        .receiver_groups
        .iter()
        .for_each(|group| assert_eq!(group, &test_group_data.creator_group));
}

#[test]
fn test_group_path_updates() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_path_updates(cs.clone()))
}

fn test_update_proposals(cipher_suite: CipherSuite) {
    println!(
        "Testing update proposals for cipher suite: {:?}",
        cipher_suite
    );

    let mut test_group_data = get_test_group(cipher_suite, 10);
    let key_lifetime = LifetimeExt::years(1, SystemTime::now()).unwrap();

    // Create an update from the ith member, have the ith + 1 member commit it
    for i in 0..test_group_data.receiver_groups.len() - 1 {
        let key_package = test_group_data.receiver_clients[i]
            .gen_key_package(&key_lifetime)
            .unwrap();

        let update_proposal = test_group_data.receiver_groups[i]
            .update_proposal(key_package)
            .unwrap();

        let update_proposal_packet = test_group_data.receiver_groups[i]
            .send_proposal(
                update_proposal,
                &test_group_data.receiver_clients[i].signature_key,
            )
            .unwrap();

        // Everyone should process the proposal
        test_group_data
            .creator_group
            .process_plaintext(update_proposal_packet.clone())
            .unwrap();

        for j in 0..test_group_data.receiver_groups.len() {
            if i != j {
                test_group_data.receiver_groups[j]
                    .process_plaintext(update_proposal_packet.clone())
                    .unwrap();
            }
        }

        // Another user will later commit the proposal
        let pending = test_group_data.receiver_groups[i + 1]
            .commit_proposals(
                vec![],
                true,
                &test_group_data.receiver_clients[i + 1].signature_key,
            )
            .unwrap();

        test_group_data
            .creator_group
            .process_plaintext(pending.plaintext.clone())
            .unwrap();

        // Everyone then receives the commit
        for j in 0..test_group_data.receiver_groups.len() {
            if i + 1 != j {
                let events = test_group_data.receiver_groups[j]
                    .process_plaintext(pending.plaintext.clone())
                    .unwrap();
                assert_eq!(events.len(), 0);
            } else {
                test_group_data.receiver_groups[j]
                    .process_pending_commit(pending.clone())
                    .unwrap();
            }
        }

        // Validate that all the groups are in the same end state
        test_group_data
            .receiver_groups
            .iter()
            .for_each(|group| assert_eq!(group, &test_group_data.creator_group));
    }
}

#[test]
fn test_group_update_proposals() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_update_proposals(cs.clone()))
}

fn test_remove_proposals(cipher_suite: CipherSuite) {
    println!(
        "Testing remove proposals for cipher suite: {:?}",
        cipher_suite
    );

    let mut test_group_data = get_test_group(cipher_suite, 10);

    // Remove people from the group one at a time
    while test_group_data.receiver_groups.len() > 1 {
        let removal = test_group_data
            .creator_group
            .remove_proposal((test_group_data.creator_group.public_tree.leaf_count() - 1) as u32)
            .unwrap();

        let pending = test_group_data
            .creator_group
            .commit_proposals(vec![removal], true, &test_group_data.creator.signature_key)
            .unwrap();

        // Process the removal in the creator group
        test_group_data
            .creator_group
            .process_pending_commit(pending.clone())
            .unwrap();

        // Process the removal in the other receiver groups
        for j in 0..test_group_data.receiver_groups.len() {
            let events = test_group_data.receiver_groups[j]
                .process_plaintext(pending.plaintext.clone())
                .unwrap();

            let removed_index = test_group_data.receiver_groups.len() - 1;
            let removed_cred = test_group_data.receiver_clients[removed_index]
                .credential
                .clone();

            assert_eq!(events.len(), 1);
            assert_eq!(events[0], Event::MembersRemoved(vec![removed_cred]))
        }

        // Validate that all the groups are in the same end state
        test_group_data
            .receiver_groups
            .remove(test_group_data.receiver_groups.len() - 1);

        test_group_data
            .receiver_groups
            .iter()
            .for_each(|group| assert_eq!(group, &test_group_data.creator_group));
    }
}

#[test]
fn test_group_remove_proposals() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_remove_proposals(cs.clone()))
}

fn test_application_messages(cipher_suite: CipherSuite, message_count: usize) {
    println!(
        "Testing application messages, cipher suite: {:?}, message count: {}",
        cipher_suite, message_count
    );

    let mut test_group_data = get_test_group(cipher_suite, 10);

    // Loop through each participant and send 5 application messages
    for i in 0..test_group_data.receiver_groups.len() {
        let test_message = b"hello world";

        for _ in 0..message_count {
            let ciphertext = test_group_data.receiver_groups[i]
                .encrypt_application_message(
                    test_message.to_vec(),
                    &test_group_data.receiver_clients[i].signature_key,
                )
                .unwrap();

            test_group_data
                .creator_group
                .process_ciphertext(ciphertext.clone())
                .unwrap();

            for j in 0..test_group_data.receiver_groups.len() {
                if i != j {
                    let events = test_group_data.receiver_groups[j]
                        .process_ciphertext(ciphertext.clone())
                        .unwrap();
                    assert_eq!(events.len(), 1);
                    assert_eq!(events[0], Event::ApplicationData(test_message.to_vec()));
                }
            }
        }
    }
}

#[test]
fn test_group_application_messages() {
    get_cipher_suites()
        .iter()
        .for_each(|cs| test_application_messages(cs.clone(), 20))
}
