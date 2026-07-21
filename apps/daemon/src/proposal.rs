use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use openchatcut_domain::{AgentCapabilityCall, Operation};
use tokio::sync::Mutex;

const MAX_PROPOSALS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalPurpose {
    Timeline,
    Script,
    AgentWorkflow,
}

#[derive(Debug, Clone)]
pub struct StoredProposal {
    pub id: String,
    pub purpose: ProposalPurpose,
    pub project_id: String,
    pub base_revision: u64,
    pub operations: Vec<Operation>,
    pub capability_calls: Vec<AgentCapabilityCall>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ProposalStore {
    inner: Arc<Mutex<HashMap<String, StoredProposal>>>,
    ttl: Duration,
}

impl ProposalStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn insert(
        &self,
        purpose: ProposalPurpose,
        project_id: &str,
        base_revision: u64,
        operations: Vec<Operation>,
    ) -> StoredProposal {
        self.insert_payload(purpose, project_id, base_revision, operations, Vec::new())
            .await
    }

    pub async fn insert_agent_workflow(
        &self,
        project_id: &str,
        base_revision: u64,
        capability_calls: Vec<AgentCapabilityCall>,
    ) -> StoredProposal {
        self.insert_payload(
            ProposalPurpose::AgentWorkflow,
            project_id,
            base_revision,
            Vec::new(),
            capability_calls,
        )
        .await
    }

    async fn insert_payload(
        &self,
        purpose: ProposalPurpose,
        project_id: &str,
        base_revision: u64,
        operations: Vec<Operation>,
        capability_calls: Vec<AgentCapabilityCall>,
    ) -> StoredProposal {
        let now = Utc::now();
        let expires_at = now
            + chrono::Duration::from_std(self.ttl)
                .expect("the bounded proposal TTL fits chrono::Duration");
        let proposal = StoredProposal {
            id: format!("proposal:{}", uuid::Uuid::new_v4()),
            purpose,
            project_id: project_id.to_owned(),
            base_revision,
            operations,
            capability_calls,
            created_at: now,
            expires_at,
        };
        let mut proposals = self.inner.lock().await;
        proposals.retain(|_, proposal| proposal.expires_at > now);
        if proposals.len() >= MAX_PROPOSALS
            && let Some(oldest) = proposals
                .values()
                .min_by_key(|proposal| proposal.created_at)
                .map(|proposal| proposal.id.clone())
        {
            proposals.remove(&oldest);
        }
        proposals.insert(proposal.id.clone(), proposal.clone());
        proposal
    }

    pub async fn get(&self, proposal_id: &str) -> Option<StoredProposal> {
        let now = Utc::now();
        let mut proposals = self.inner.lock().await;
        proposals.retain(|_, proposal| proposal.expires_at > now);
        proposals.get(proposal_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn proposals_expire() {
        let store = ProposalStore::new(Duration::ZERO);
        let proposal = store
            .insert(
                ProposalPurpose::Timeline,
                "project",
                1,
                vec![Operation::SetProjectName {
                    name: "New name".to_owned(),
                }],
            )
            .await;
        assert!(store.get(&proposal.id).await.is_none());
    }

    #[tokio::test]
    async fn workflow_proposals_cannot_be_applied_as_timeline_operations() {
        let store = ProposalStore::new(Duration::from_secs(60));
        let proposal = store
            .insert_agent_workflow(
                "project",
                1,
                vec![AgentCapabilityCall::StartTranscription {
                    asset_id: "asset:voice".to_owned(),
                    language: None,
                    diarization: false,
                    min_speakers: None,
                    max_speakers: None,
                    engine: None,
                }],
            )
            .await;
        assert_eq!(proposal.purpose, ProposalPurpose::AgentWorkflow);
        assert!(proposal.operations.is_empty());
        assert_eq!(proposal.capability_calls.len(), 1);
    }
}
